pub mod raw;

use std::io::{Error, Result};
use std::path::Path;

pub mod rust_cfg;
use rust_cfg::new_sdb_data_config;
pub use rust_cfg::{CompareResult, DefaultSdb, SdbMessage, SdbMessageType, SdbRustDataFuncs};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug)]
pub struct DBConfig {
    pub cache_size_bytes: usize,
    pub disk_size_bytes: usize,
    pub max_key_size: usize,
    pub max_value_size: usize,
}

#[derive(Debug)]
pub struct SplinterDB {
    _inner: *mut raw::splinterdb,
    sdb_cfg: Box<raw::splinterdb_config>,
    data_cfg: Box<raw::data_config>,
}

unsafe impl Sync for SplinterDB {}
unsafe impl Send for SplinterDB {}

impl Drop for SplinterDB {
    fn drop(&mut self) {
        unsafe { raw::splinterdb_close(&mut self._inner) };
    }
}

#[derive(Debug, PartialEq)]
pub enum LookupResult {
    Found(Vec<u8>),
    FoundTruncated(Vec<u8>),
    NotFound,
}

fn as_result(rc: ::std::os::raw::c_int) -> Result<()> {
    if rc != 0 {
        Err(Error::from_raw_os_error(rc))
    } else {
        Ok(())
    }
}

fn create_splinter_slice(ref v: &[u8]) -> raw::slice {
    unsafe {
        raw::slice {
            length: v.len() as u64,
            data: ::std::mem::transmute(v.as_ptr()),
        }
    }
}

#[derive(Debug)]
pub struct IteratorResult<'a> {
    pub key: &'a [u8],
    pub value: &'a [u8],
}

#[derive(Debug)]
pub struct SplinterCursor<'a> {
    _inner: *mut raw::splinterdb_iterator,
    _marker: ::std::marker::PhantomData<raw::splinterdb_iterator>,
    _parent_marker: ::std::marker::PhantomData<&'a raw::splinterdb>,
    state: Option<IteratorResult<'a>>,
}

impl<'a> Drop for SplinterCursor<'a> {
    fn drop(&mut self) {
        unsafe { raw::splinterdb_iterator_deinit(self._inner) }
    }
}

// Bidirectional cursor for SplinterDB
// can return errors and retains ownership of the result
impl<'a> SplinterCursor<'a> {
    pub fn new(iter: *mut raw::splinterdb_iterator) -> Result<SplinterCursor<'a>> {
        Ok(SplinterCursor {
            _inner: iter,
            _marker: ::std::marker::PhantomData,
            _parent_marker: ::std::marker::PhantomData,
            state: Self::_get_current(iter)?,
        })
    }

    // returns the current state of the iterator from the C API
    fn _get_current(it: *mut raw::splinterdb_iterator) -> Result<Option<IteratorResult<'a>>> {
        let valid: i32 = unsafe { raw::splinterdb_iterator_valid(it) } as i32;

        if valid == 0 {
            // cannot access the current element, check status
            let rc = unsafe { raw::splinterdb_iterator_status(it) };
            as_result(rc)?;
            return Ok(None);
        }

        let mut key_out: raw::slice = raw::slice {
            length: 0,
            data: ::std::ptr::null(),
        };
        let mut val_out: raw::slice = raw::slice {
            length: 0,
            data: ::std::ptr::null(),
        };

        let (key, value): (&[u8], &[u8]) = unsafe {
            // get key and value
            raw::splinterdb_iterator_get_current(it, &mut key_out, &mut val_out);
            // parse key and value into rust slices
            (
                ::std::slice::from_raw_parts(
                    ::std::mem::transmute(key_out.data),
                    key_out.length as usize,
                ),
                ::std::slice::from_raw_parts(
                    ::std::mem::transmute(val_out.data),
                    val_out.length as usize,
                ),
            )
        };
        let r = IteratorResult { key, value };
        Ok(Some(r))
    }

    pub fn get_curr(&self) -> Option<&IteratorResult> {
        match self.state {
            None => None,
            Some(ref r) => Some(r),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<bool> {
        let can_next = unsafe { raw::splinterdb_iterator_can_next(self._inner) } as i32;
        if can_next == 0 {
            let rc = unsafe { raw::splinterdb_iterator_status(self._inner) };
            as_result(rc)?;
            return Ok(false);
        }
        unsafe {
            raw::splinterdb_iterator_next(self._inner);
        }

        self.state = Self::_get_current(self._inner)?;
        Ok(true)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn prev(&mut self) -> Result<bool> {
        let can_prev = unsafe { raw::splinterdb_iterator_can_prev(self._inner) } as i32;
        if can_prev == 0 {
            let rc = unsafe { raw::splinterdb_iterator_status(self._inner) };
            as_result(rc)?;
            return Ok(false);
        }
        unsafe {
            raw::splinterdb_iterator_prev(self._inner);
        }

        self.state = Self::_get_current(self._inner)?;
        Ok(true)
    }
}

fn path_as_cstring<P: AsRef<Path>>(path: P) -> std::ffi::CString {
    let as_os_str = path.as_ref().as_os_str();
    let as_str = as_os_str.to_str().unwrap();
    std::ffi::CString::new(as_str).unwrap()
}

pub const O_DIRECT: std::os::raw::c_int = 0x4000;

impl SplinterDB {
    // Create a new SplinterDB object. This is uninitialized.
    pub fn new<T: rust_cfg::SdbRustDataFuncs>() -> SplinterDB {
        SplinterDB {
            _inner: std::ptr::null_mut(),
            sdb_cfg: Box::new(unsafe { std::mem::zeroed() }),
            data_cfg: Box::new(new_sdb_data_config::<T>(0)),
        }
    }

    fn db_create_or_open<P: AsRef<Path>>(
        &mut self,
        path: &P,
        cfg: &DBConfig,
        open_existing: bool,
    ) -> Result<()> {
        let path = path_as_cstring(path); // don't drop until init is done

        // set up the splinterdb config
        self.sdb_cfg.filename = path.as_ptr();
        self.sdb_cfg.cache_size = cfg.cache_size_bytes as u64;
        self.sdb_cfg.disk_size = cfg.disk_size_bytes as u64;
        self.sdb_cfg.data_cfg = self.data_cfg.as_mut();
        self.sdb_cfg.num_memtable_bg_threads = 2;
        self.sdb_cfg.num_normal_bg_threads = 2;
        self.sdb_cfg.io_flags |= O_DIRECT;

        // set key bytes
        self.data_cfg.max_key_size = cfg.max_key_size as u64;

        // Open or create the database
        let rc = if open_existing {
            unsafe { raw::splinterdb_open(self.sdb_cfg.as_ref(), &mut self._inner) }
        } else {
            unsafe { raw::splinterdb_create(self.sdb_cfg.as_ref(), &mut self._inner) }
        };
        as_result(rc)
    }

    pub fn db_create<P: AsRef<Path>>(&mut self, path: &P, cfg: &DBConfig) -> Result<()> {
        self.db_create_or_open(path, cfg, false)
    }

    pub fn db_open<P: AsRef<Path>>(&mut self, path: &P, cfg: &DBConfig) -> Result<()> {
        self.db_create_or_open(path, cfg, true)
    }

    pub fn register_thread(&self) {
        unsafe { raw::splinterdb_register_thread(self._inner) };
    }

    pub fn deregister_thread(&self) {
        unsafe { raw::splinterdb_deregister_thread(self._inner) };
    }

    pub fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let key_slice: raw::slice = create_splinter_slice(key);
        let val_slice: raw::slice = create_splinter_slice(value);

        let rc = unsafe { raw::splinterdb_insert(self._inner, key_slice, val_slice) };
        as_result(rc)
    }

    pub fn update(&self, key: &[u8], delta: &[u8]) -> Result<()> {
        let key_slice: raw::slice = create_splinter_slice(key);
        let delta_slice: raw::slice = create_splinter_slice(delta);

        let rc = unsafe { raw::splinterdb_update(self._inner, key_slice, delta_slice) };
        as_result(rc)
    }

    pub fn delete(&self, key: &[u8]) -> Result<()> {
        let rc = unsafe { raw::splinterdb_delete(self._inner, create_splinter_slice(key)) };
        as_result(rc)
    }

    pub fn lookup(&self, key: &[u8]) -> Result<LookupResult> {
        unsafe {
            let mut lr: raw::splinterdb_lookup_result = std::mem::zeroed();
            raw::splinterdb_lookup_result_init(self._inner, &mut lr, 0, std::ptr::null_mut());

            let rc = raw::splinterdb_lookup(self._inner, create_splinter_slice(key), &mut lr);
            as_result(rc)?;

            let found = raw::splinterdb_lookup_found(&lr) as i32;
            if found == 0 {
                return Ok(LookupResult::NotFound);
            }

            let mut val: raw::slice = raw::slice {
                length: 0,
                data: std::mem::zeroed(),
            };
            let rc = raw::splinterdb_lookup_result_value(&lr, &mut val);
            as_result(rc)?;

            // TODO: Can we avoid this memory init and copy?
            let mut value: Vec<u8> = vec![0; val.length as usize];
            std::ptr::copy(
                val.data,
                std::mem::transmute(value.as_mut_ptr()),
                val.length as usize,
            );
            Ok(LookupResult::Found(value))
        }
    }

    pub fn range(&self, start_key: Option<&[u8]>) -> Result<SplinterCursor> {
        let mut iter: *mut raw::splinterdb_iterator = std::ptr::null_mut();

        let rc = unsafe {
            let start_slice: raw::slice = match start_key {
                Some(s) => raw::slice {
                    length: s.len() as u64,
                    data: ::std::mem::transmute(s.as_ptr()),
                },
                None => raw::slice {
                    length: 0,
                    data: ::std::ptr::null(),
                },
            };
            raw::splinterdb_iterator_init(self._inner, &mut iter, start_slice)
        };
        as_result(rc)?;
        return SplinterCursor::new(iter);
    }
}

mod tests;
