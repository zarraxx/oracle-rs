#![allow(missing_docs, non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_int, c_long, c_uint, c_void};
use std::ptr;
use std::sync::{Mutex, OnceLock};

/*
 * Minimal C ABI surface for integrating oracle-rs as a static library backend.
 * These functions are currently placeholders so the parent project can link
 * against a stable symbol set while the real implementation is built out.
 */

#[repr(C)]
#[derive(Clone, Copy)]
pub enum oraType {
    ORA_TYPE_VARCHAR2,
    ORA_TYPE_CHAR,
    ORA_TYPE_NVARCHAR2,
    ORA_TYPE_NCHAR,
    ORA_TYPE_NUMBER,
    ORA_TYPE_FLOAT,
    ORA_TYPE_BINARYFLOAT,
    ORA_TYPE_BINARYDOUBLE,
    ORA_TYPE_RAW,
    ORA_TYPE_DATE,
    ORA_TYPE_TIMESTAMP,
    ORA_TYPE_TIMESTAMPTZ,
    ORA_TYPE_TIMESTAMPLTZ,
    ORA_TYPE_INTERVALY2M,
    ORA_TYPE_INTERVALD2S,
    ORA_TYPE_BLOB,
    ORA_TYPE_CLOB,
    ORA_TYPE_NCLOB,
    ORA_TYPE_BFILE,
    ORA_TYPE_LONG,
    ORA_TYPE_LONGRAW,
    ORA_TYPE_GEOMETRY,
    ORA_TYPE_XMLTYPE,
    ORA_TYPE_OTHER,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum oraIsoLevel {
    ORA_TRANS_READ_COMMITTED,
    ORA_TRANS_READ_ONLY,
    ORA_TRANS_SERIALIZABLE,
}

#[repr(C)]
pub struct oraTable {
    _private: [u8; 0],
}

#[repr(C)]
pub struct paramDesc {
    _private: [u8; 0],
}

#[repr(C)]
pub struct oracleSession {
    pub thin_conn: *mut c_void,
    pub thin_stmt: *mut c_void,
    pub thin_result: *mut c_void,
    pub thin_runtime: *mut c_void,
    pub user_data: *mut c_void,
    pub have_nchar: c_int,
    pub server_version: [c_int; 5],
    pub last_batch: c_uint,
    pub fetched_rows: c_uint,
    pub current_row: c_uint,
}

impl Default for oracleSession {
    fn default() -> Self {
        Self {
            thin_conn: ptr::null_mut(),
            thin_stmt: ptr::null_mut(),
            thin_result: ptr::null_mut(),
            thin_runtime: ptr::null_mut(),
            user_data: ptr::null_mut(),
            have_nchar: 0,
            server_version: [0; 5],
            last_batch: 0,
            fetched_rows: 0,
            current_row: 0,
        }
    }
}

fn session_registry() -> &'static Mutex<Vec<usize>> {
    static REGISTRY: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn register_session(session: *mut oracleSession) {
    if let Ok(mut sessions) = session_registry().lock() {
        sessions.push(session as usize);
    }
}

fn clear_sessions() {
    if let Ok(mut sessions) = session_registry().lock() {
        for session in sessions.drain(..) {
            let session = session as *mut oracleSession;
            if !session.is_null() {
                unsafe {
                    drop(Box::from_raw(session));
                }
            }
        }
    }
}

fn set_output_int(ptr_out: *mut c_int, value: c_int) {
    if !ptr_out.is_null() {
        unsafe {
            *ptr_out = value;
        }
    }
}

fn set_output_long(ptr_out: *mut c_long, value: c_long) {
    if !ptr_out.is_null() {
        unsafe {
            *ptr_out = value;
        }
    }
}

fn set_output_ptr<T>(ptr_out: *mut *mut T, value: *mut T) {
    if !ptr_out.is_null() {
        unsafe {
            *ptr_out = value;
        }
    }
}

fn set_version_fields(
    major: *mut c_int,
    minor: *mut c_int,
    update: *mut c_int,
    patch: *mut c_int,
    port_patch: *mut c_int,
    values: [c_int; 5],
) {
    set_output_int(major, values[0]);
    set_output_int(minor, values[1]);
    set_output_int(update, values[2]);
    set_output_int(patch, values[3]);
    set_output_int(port_patch, values[4]);
}

#[no_mangle]
pub extern "C" fn oracleGetSession(
    _connectstring: *const c_char,
    _isolation_level: oraIsoLevel,
    _user: *mut c_char,
    _password: *mut c_char,
    _nls_lang: *const c_char,
    _timezone: *const c_char,
    have_nchar: c_int,
    _tablename: *const c_char,
    _curlevel: c_int,
) -> *mut oracleSession {
    let session = Box::new(oracleSession {
        have_nchar,
        ..oracleSession::default()
    });
    let session = Box::into_raw(session);
    register_session(session);
    session
}

#[no_mangle]
pub extern "C" fn oracleCloseStatement(session: *mut oracleSession) {
    if session.is_null() {
        return;
    }

    unsafe {
        (*session).thin_stmt = ptr::null_mut();
        (*session).thin_result = ptr::null_mut();
        (*session).last_batch = 0;
        (*session).fetched_rows = 0;
        (*session).current_row = 0;
    }
}

#[no_mangle]
pub extern "C" fn oracleCloseConnections() {
    clear_sessions();
}

#[no_mangle]
pub extern "C" fn oracleShutdown() {
    clear_sessions();
}

#[no_mangle]
pub extern "C" fn oracleCancel() {}

#[no_mangle]
pub extern "C" fn oracleEndTransaction(_arg: *mut c_void, _is_commit: c_int, _silent: c_int) {}

#[no_mangle]
pub extern "C" fn oracleEndSubtransaction(_arg: *mut c_void, _nest_level: c_int, _is_commit: c_int) {}

#[no_mangle]
pub extern "C" fn oracleIsStatementOpen(session: *mut oracleSession) -> c_int {
    if session.is_null() {
        return 0;
    }

    unsafe { (!(*session).thin_stmt.is_null()) as c_int }
}

#[no_mangle]
pub extern "C" fn oracleDescribe(
    _session: *mut oracleSession,
    _dblink: *mut c_char,
    _schema: *mut c_char,
    _table: *mut c_char,
    _pgname: *mut c_char,
    _max_long: c_long,
    has_geometry: *mut c_int,
) -> *mut oraTable {
    set_output_int(has_geometry, 0);
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn oracleExplain(
    _session: *mut oracleSession,
    _query: *const c_char,
    nrows: *mut c_int,
    plan: *mut *mut *mut c_char,
) {
    set_output_int(nrows, 0);
    set_output_ptr(plan, ptr::null_mut());
}

#[no_mangle]
pub extern "C" fn oraclePrepareQuery(
    _session: *mut oracleSession,
    _query: *const c_char,
    _oraTable: *const oraTable,
    _prefetch: c_uint,
    _lob_prefetch: c_uint,
) {
}

#[no_mangle]
pub extern "C" fn oracleExecuteQuery(
    session: *mut oracleSession,
    _oraTable: *const oraTable,
    _paramList: *mut paramDesc,
    _prefetch: c_uint,
) -> c_uint {
    if session.is_null() {
        return 0;
    }

    unsafe {
        (*session).last_batch = 0;
        (*session).fetched_rows = 0;
        (*session).current_row = 0;
    }

    0
}

#[no_mangle]
pub extern "C" fn oracleFetchNext(session: *mut oracleSession, _prefetch: c_uint) -> c_uint {
    if session.is_null() {
        return 0;
    }

    unsafe {
        (*session).current_row = 0;
    }

    0
}

#[no_mangle]
pub extern "C" fn oracleExecuteCall(_session: *mut oracleSession, _stmt: *mut c_char) {}

#[no_mangle]
pub extern "C" fn oracleGetLob(
    _session: *mut oracleSession,
    _locptr: *mut c_void,
    _type: oraType,
    value: *mut *mut c_char,
    value_len: *mut c_long,
) {
    set_output_ptr(value, ptr::null_mut());
    set_output_long(value_len, 0);
}

#[no_mangle]
pub extern "C" fn oracleClientVersion(
    major: *mut c_int,
    minor: *mut c_int,
    update: *mut c_int,
    patch: *mut c_int,
    port_patch: *mut c_int,
) {
    set_version_fields(major, minor, update, patch, port_patch, [0, 1, 6, 0, 0]);
}

#[no_mangle]
pub extern "C" fn oracleServerVersion(
    session: *mut oracleSession,
    major: *mut c_int,
    minor: *mut c_int,
    update: *mut c_int,
    patch: *mut c_int,
    port_patch: *mut c_int,
) {
    if session.is_null() {
        set_version_fields(major, minor, update, patch, port_patch, [0; 5]);
        return;
    }

    unsafe {
        set_version_fields(
            major,
            minor,
            update,
            patch,
            port_patch,
            (*session).server_version,
        );
    }
}

#[no_mangle]
pub extern "C" fn oracleGetGeometryType(_session: *mut oracleSession) -> *mut c_void {
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn oracleGetImportColumn(
    _session: *mut oracleSession,
    _dblink: *mut c_char,
    _schema: *mut c_char,
    _limit_to: *mut c_char,
    tabname: *mut *mut c_char,
    colname: *mut *mut c_char,
    type_out: *mut oraType,
    charlen: *mut c_int,
    typeprec: *mut c_int,
    typescale: *mut c_int,
    nullable: *mut c_int,
    key: *mut c_int,
    _skip_tables: c_int,
    _skip_views: c_int,
    _skip_matviews: c_int,
) -> c_int {
    set_output_ptr(tabname, ptr::null_mut());
    set_output_ptr(colname, ptr::null_mut());

    if !type_out.is_null() {
        unsafe {
            *type_out = oraType::ORA_TYPE_OTHER;
        }
    }

    set_output_int(charlen, 0);
    set_output_int(typeprec, 0);
    set_output_int(typescale, 0);
    set_output_int(nullable, 0);
    set_output_int(key, 0);
    0
}
