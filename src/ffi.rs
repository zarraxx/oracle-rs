#![allow(missing_docs, non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_int, c_long, c_uint, c_void, CStr, CString};
use std::future::Future;
use std::mem;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use tokio::runtime::{Builder, Runtime};

use crate::config::Config;
use crate::connection::{Connection, QueryResult};
use crate::error::Error;
use crate::row::{Row, Value};
use crate::types::{LobData, LobValue};

const ORACLE_FDW_PG_CALLBACKS_VERSION: c_uint = 1;
const ORACLE_RS_CLIENT_VERSION: [c_int; 5] = [0, 1, 6, 0, 0];

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy)]
pub enum oraError {
    FDW_ERROR,
    FDW_UNABLE_TO_ESTABLISH_CONNECTION,
    FDW_UNABLE_TO_CREATE_REPLY,
    FDW_UNABLE_TO_CREATE_EXECUTION,
    FDW_TABLE_NOT_FOUND,
    FDW_OUT_OF_MEMORY,
    FDW_SERIALIZATION_FAILURE,
    FDW_UNIQUE_VIOLATION,
    FDW_DEADLOCK_DETECTED,
    FDW_NOT_NULL_VIOLATION,
    FDW_CHECK_VIOLATION,
    FDW_FOREIGN_KEY_VIOLATION,
}

#[repr(C)]
pub struct oraColumn {
    pub name: *mut c_char,
    pub oratype: oraType,
    pub scale: c_int,
    pub pgname: *mut c_char,
    pub pgattnum: c_int,
    pub pgtype: c_uint,
    pub pgtypmod: c_int,
    pub used: c_int,
    pub strip_zeros: c_int,
    pub pkey: c_int,
    pub val: *mut c_char,
    pub val_size: i32,
    pub val_len: *mut u16,
    pub val_null: *mut i16,
    pub val_len4: u32,
    pub varno: c_int,
}

#[repr(C)]
pub struct oraTable {
    pub name: *mut c_char,
    pub pgname: *mut c_char,
    pub ncols: c_int,
    pub npgcols: c_int,
    pub cols: *mut *mut oraColumn,
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

type PgAlloc = unsafe extern "C" fn(size: usize) -> *mut c_void;
type PgRealloc = unsafe extern "C" fn(ptr: *mut c_void, size: usize) -> *mut c_void;
type PgFree = unsafe extern "C" fn(ptr: *mut c_void);
type PgError = unsafe extern "C" fn(sqlstate: oraError, message: *const c_char);
type PgErrorI = unsafe extern "C" fn(sqlstate: oraError, message: *const c_char, arg: c_int);
type PgErrorII =
    unsafe extern "C" fn(sqlstate: oraError, message: *const c_char, arg1: c_int, arg2: c_int);
type PgErrorD =
    unsafe extern "C" fn(sqlstate: oraError, message: *const c_char, detail: *const c_char);
type PgErrorSD = unsafe extern "C" fn(
    sqlstate: oraError,
    message: *const c_char,
    arg: *const c_char,
    detail: *const c_char,
);
type PgErrorSSDH = unsafe extern "C" fn(
    sqlstate: oraError,
    message: *const c_char,
    arg1: *const c_char,
    arg2: *const c_char,
    detail: *const c_char,
    hint: *const c_char,
);
type PgDebug2 = unsafe extern "C" fn(message: *const c_char);
type PgSetHandlers = unsafe extern "C" fn();
type PgRegisterCallback = unsafe extern "C" fn(arg: *mut c_void);
type PgUnregisterCallback = unsafe extern "C" fn(arg: *mut c_void);
type PgInitializePostgis = unsafe extern "C" fn();
type PgGetShareFileName = unsafe extern "C" fn(relativename: *const c_char) -> *mut c_char;
type PgEwkbToGeom = unsafe extern "C" fn(
    session: *mut oracleSession,
    ewkb_length: c_uint,
    ewkb_data: *mut c_char,
) -> *mut c_void;
type PgGetEwkbLen = unsafe extern "C" fn(session: *mut oracleSession, geom: *mut c_void) -> c_uint;
type PgFillEwkb = unsafe extern "C" fn(
    session: *mut oracleSession,
    geom: *mut c_void,
    size: c_uint,
    dest: *mut c_char,
) -> *mut c_char;
type PgGeometryFree = unsafe extern "C" fn(session: *mut oracleSession, geom: *mut c_void);
type PgGeometryAlloc = unsafe extern "C" fn(session: *mut oracleSession, geom: *mut c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OracleFdwPgCoreCallbacks {
    pub alloc: Option<PgAlloc>,
    pub realloc: Option<PgRealloc>,
    pub free: Option<PgFree>,
    pub error: Option<PgError>,
    pub error_i: Option<PgErrorI>,
    pub error_ii: Option<PgErrorII>,
    pub error_d: Option<PgErrorD>,
    pub error_sd: Option<PgErrorSD>,
    pub error_ssdh: Option<PgErrorSSDH>,
    pub debug2: Option<PgDebug2>,
    pub set_handlers: Option<PgSetHandlers>,
    pub register_callback: Option<PgRegisterCallback>,
    pub unregister_callback: Option<PgUnregisterCallback>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OracleFdwPgPostgisCallbacks {
    pub initialize_postgis: Option<PgInitializePostgis>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OracleFdwPgGeometryCallbacks {
    pub get_share_file_name: Option<PgGetShareFileName>,
    pub ewkb_to_geom: Option<PgEwkbToGeom>,
    pub get_ewkb_len: Option<PgGetEwkbLen>,
    pub fill_ewkb: Option<PgFillEwkb>,
    pub geometry_free: Option<PgGeometryFree>,
    pub geometry_alloc: Option<PgGeometryAlloc>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OracleFdwPgCallbacks {
    pub version: c_uint,
    pub core: OracleFdwPgCoreCallbacks,
    pub postgis: OracleFdwPgPostgisCallbacks,
    pub geometry: OracleFdwPgGeometryCallbacks,
}

struct RustLobContent {
    data: Vec<u8>,
}

struct ImportColumn {
    tabname: String,
    colname: String,
    typename: String,
    typeowner: Option<String>,
    charlen: c_int,
    typeprec: c_int,
    typescale: c_int,
    nullable: c_int,
    key: c_int,
}

struct RustSessionState {
    conn: Connection,
    current_sql: Option<String>,
    current_table: *const oraTable,
    current_result: Option<QueryResult>,
    next_row: usize,
    import_rows: Option<Vec<ImportColumn>>,
    import_next: usize,
    lob_handles: Vec<*mut RustLobContent>,
    xact_level: c_int,
    readonly: bool,
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build oracle-rs FFI runtime")
    })
}

fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(future)
}

fn pg_callbacks_registry() -> &'static Mutex<Option<OracleFdwPgCallbacks>> {
    static REGISTRY: OnceLock<Mutex<Option<OracleFdwPgCallbacks>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(None))
}

fn pg_callbacks() -> Option<OracleFdwPgCallbacks> {
    pg_callbacks_registry()
        .lock()
        .ok()
        .and_then(|callbacks| *callbacks)
}

fn has_required_core_callbacks(callbacks: &OracleFdwPgCallbacks) -> bool {
    callbacks.core.alloc.is_some()
        && callbacks.core.realloc.is_some()
        && callbacks.core.free.is_some()
        && callbacks.core.error.is_some()
        && callbacks.core.error_i.is_some()
        && callbacks.core.error_ii.is_some()
        && callbacks.core.error_d.is_some()
        && callbacks.core.error_sd.is_some()
        && callbacks.core.error_ssdh.is_some()
        && callbacks.core.debug2.is_some()
        && callbacks.core.set_handlers.is_some()
        && callbacks.core.register_callback.is_some()
        && callbacks.core.unregister_callback.is_some()
}

unsafe fn pg_alloc(size: usize) -> *mut c_void {
    pg_callbacks()
        .and_then(|callbacks| callbacks.core.alloc)
        .map_or(ptr::null_mut(), |alloc| alloc(size))
}

unsafe fn pg_debug2(message: &str) {
    if let Some(debug2) = pg_callbacks().and_then(|callbacks| callbacks.core.debug2) {
        if let Ok(message) = CString::new(message) {
            debug2(message.as_ptr());
        }
    }
}

unsafe fn pg_initialize_postgis() {
    if let Some(initialize_postgis) =
        pg_callbacks().and_then(|callbacks| callbacks.postgis.initialize_postgis)
    {
        initialize_postgis();
    }
}

fn pg_callbacks_registered() -> bool {
    pg_callbacks_registry()
        .lock()
        .map(|callbacks| callbacks.is_some())
        .unwrap_or(false)
}

unsafe fn raise_pg_error(sqlstate: oraError, message: &str, detail: impl ToString) -> ! {
    let message =
        CString::new(message).unwrap_or_else(|_| CString::new("oracle-rs error").unwrap());
    let detail_text = detail.to_string();
    let detail = CString::new(detail_text)
        .unwrap_or_else(|_| CString::new("error detail contained an embedded NUL byte").unwrap());

    if let Some(error_d) = pg_callbacks().and_then(|callbacks| callbacks.core.error_d) {
        error_d(sqlstate, message.as_ptr(), detail.as_ptr());
    }

    std::process::abort();
}

unsafe fn alloc_zeroed<T>() -> *mut T {
    let ptr = pg_alloc(mem::size_of::<T>()) as *mut T;
    if ptr.is_null() {
        raise_pg_error(
            oraError::FDW_OUT_OF_MEMORY,
            "oracle-rs failed to allocate memory",
            format!("{} bytes", mem::size_of::<T>()),
        );
    }
    ptr::write_bytes(ptr, 0, 1);
    ptr
}

unsafe fn alloc_array<T>(count: usize) -> *mut T {
    let size = mem::size_of::<T>().saturating_mul(count);
    let ptr = pg_alloc(size) as *mut T;
    if ptr.is_null() {
        raise_pg_error(
            oraError::FDW_OUT_OF_MEMORY,
            "oracle-rs failed to allocate memory",
            format!("{} bytes", size),
        );
    }
    ptr::write_bytes(ptr, 0, count);
    ptr
}

unsafe fn alloc_c_string(value: &str) -> *mut c_char {
    let bytes = value.as_bytes();
    let ptr = pg_alloc(bytes.len() + 1) as *mut c_char;
    if ptr.is_null() {
        raise_pg_error(
            oraError::FDW_OUT_OF_MEMORY,
            "oracle-rs failed to allocate string memory",
            bytes.len() + 1,
        );
    }
    ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, ptr, bytes.len());
    *ptr.add(bytes.len()) = 0;
    ptr
}

unsafe fn set_output_int(ptr_out: *mut c_int, value: c_int) {
    if !ptr_out.is_null() {
        *ptr_out = value;
    }
}

unsafe fn set_output_long(ptr_out: *mut c_long, value: c_long) {
    if !ptr_out.is_null() {
        *ptr_out = value;
    }
}

unsafe fn set_output_ptr<T>(ptr_out: *mut *mut T, value: *mut T) {
    if !ptr_out.is_null() {
        *ptr_out = value;
    }
}

unsafe fn cstr_to_string(ptr_in: *const c_char) -> String {
    if ptr_in.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr_in).to_string_lossy().into_owned()
    }
}

fn quote_ident(identifier: &str) -> String {
    let mut out = String::with_capacity(identifier.len() + 2);
    out.push('"');
    for ch in identifier.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn sql_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

fn normalize_connect_string(connectstring: &str) -> String {
    connectstring.trim_start_matches('/').to_string()
}

fn parse_version_numbers(version: &str) -> [c_int; 5] {
    let mut parsed = [0; 5];
    for (idx, part) in version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .take(5)
        .enumerate()
    {
        parsed[idx] = part.parse::<c_int>().unwrap_or(0);
    }
    parsed
}

fn ora_type_from_names(typename: &str, typeowner: Option<&str>) -> oraType {
    let typename = typename.trim().to_ascii_uppercase();
    let typeowner = typeowner.unwrap_or("").trim().to_ascii_uppercase();

    if typename.starts_with("VARCHAR") {
        oraType::ORA_TYPE_VARCHAR2
    } else if typename == "NUMBER" {
        oraType::ORA_TYPE_NUMBER
    } else if typename == "DATE" {
        oraType::ORA_TYPE_DATE
    } else if typename == "CHAR" {
        oraType::ORA_TYPE_CHAR
    } else if typename.starts_with("TIMESTAMP") {
        if typename.len() < 17 {
            oraType::ORA_TYPE_TIMESTAMP
        } else if typename.contains("LOCAL") {
            oraType::ORA_TYPE_TIMESTAMPLTZ
        } else {
            oraType::ORA_TYPE_TIMESTAMPTZ
        }
    } else if typename == "RAW" {
        oraType::ORA_TYPE_RAW
    } else if typename == "BLOB" {
        oraType::ORA_TYPE_BLOB
    } else if typename == "CLOB" {
        oraType::ORA_TYPE_CLOB
    } else if typename == "NCLOB" {
        oraType::ORA_TYPE_NCLOB
    } else if typename == "BFILE" {
        oraType::ORA_TYPE_BFILE
    } else if typename == "LONG" {
        oraType::ORA_TYPE_LONG
    } else if typename == "LONG RAW" {
        oraType::ORA_TYPE_LONGRAW
    } else if typename == "SDO_GEOMETRY" && typeowner == "MDSYS" {
        oraType::ORA_TYPE_GEOMETRY
    } else if typename == "XMLTYPE" && (typeowner == "PUBLIC" || typeowner == "SYS") {
        oraType::ORA_TYPE_XMLTYPE
    } else if typename == "FLOAT" {
        oraType::ORA_TYPE_FLOAT
    } else if typename.starts_with("NVARCHAR") {
        oraType::ORA_TYPE_NVARCHAR2
    } else if typename == "NCHAR" {
        oraType::ORA_TYPE_NCHAR
    } else if typename.starts_with("INTERVAL DAY") {
        oraType::ORA_TYPE_INTERVALD2S
    } else if typename.starts_with("INTERVAL YEAR") {
        oraType::ORA_TYPE_INTERVALY2M
    } else if typename == "BINARY_FLOAT" {
        oraType::ORA_TYPE_BINARYFLOAT
    } else if typename == "BINARY_DOUBLE" {
        oraType::ORA_TYPE_BINARYDOUBLE
    } else {
        oraType::ORA_TYPE_OTHER
    }
}

fn value_as_string(row: &Row, idx: usize) -> Option<String> {
    match row.get(idx)? {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        Value::Integer(value) => Some(value.to_string()),
        Value::Float(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.as_str().to_string()),
        value => Some(value.to_string()),
    }
}

fn value_as_i32(row: &Row, idx: usize) -> Option<c_int> {
    row.get(idx)
        .and_then(Value::as_i64)
        .map(|value| value as c_int)
        .or_else(|| {
            value_as_string(row, idx).and_then(|value| {
                let value = value.trim();
                if value.is_empty() || value.eq_ignore_ascii_case("NULL") {
                    None
                } else {
                    value.parse::<c_int>().ok()
                }
            })
        })
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

unsafe fn state_mut(session: *mut oracleSession) -> &'static mut RustSessionState {
    if session.is_null() || (*session).thin_runtime.is_null() {
        raise_pg_error(
            oraError::FDW_ERROR,
            "oracle-rs internal error: invalid Oracle session",
            "session pointer is NULL",
        );
    }

    &mut *((*session).thin_runtime as *mut RustSessionState)
}

unsafe fn close_statement_state(session: *mut oracleSession) {
    if session.is_null() || (*session).thin_runtime.is_null() {
        return;
    }

    let state = state_mut(session);
    state.current_sql = None;
    state.current_table = ptr::null();
    state.current_result = None;
    state.next_row = 0;
    state.import_rows = None;
    state.import_next = 0;
    for handle in state.lob_handles.drain(..) {
        drop(Box::from_raw(handle));
    }

    (*session).thin_stmt = ptr::null_mut();
    (*session).thin_result = ptr::null_mut();
    (*session).last_batch = 0;
    (*session).fetched_rows = 0;
    (*session).current_row = 0;
}

unsafe fn drop_session(session: *mut oracleSession) {
    if session.is_null() {
        return;
    }

    close_statement_state(session);

    if !(*session).thin_runtime.is_null() {
        let mut state = Box::from_raw((*session).thin_runtime as *mut RustSessionState);
        let conn = &state.conn;
        let _ = block_on(async { conn.close().await });
        state.thin_drop_lobs();
        (*session).thin_runtime = ptr::null_mut();
    }

    drop(Box::from_raw(session));
}

impl RustSessionState {
    fn thin_drop_lobs(&mut self) {
        for handle in self.lob_handles.drain(..) {
            unsafe {
                drop(Box::from_raw(handle));
            }
        }
    }
}

unsafe fn clear_sessions() {
    if let Ok(mut sessions) = session_registry().lock() {
        for session in sessions.drain(..) {
            drop_session(session as *mut oracleSession);
        }
    }
}

async fn query_server_version(conn: &Connection) -> [c_int; 5] {
    for sql in [
        "SELECT version_full FROM product_component_version WHERE product LIKE 'Oracle Database%'",
        "SELECT version FROM product_component_version WHERE product LIKE 'Oracle Database%'",
        "SELECT banner FROM v$version WHERE banner LIKE 'Oracle Database%'",
    ] {
        if let Ok(result) = conn.query(sql, &[]).await {
            if let Some(version) = result.first().and_then(|row| value_as_string(row, 0)) {
                return parse_version_numbers(&version);
            }
        }
    }

    parse_version_numbers(&conn.server_info().await.version)
}

unsafe fn copy_value_to_column(
    session: *mut oracleSession,
    column: *mut oraColumn,
    slot: usize,
    value: &Value,
) {
    if column.is_null() {
        return;
    }

    let state = state_mut(session);
    let val_null = (*column).val_null;
    let val_len = (*column).val_len;

    if value.is_null() {
        if !val_null.is_null() {
            *val_null.add(slot) = -1;
        }
        if !val_len.is_null() {
            *val_len.add(slot) = 0;
        }
        return;
    }

    if !val_null.is_null() {
        *val_null.add(slot) = 0;
    }

    match (*column).oratype {
        oraType::ORA_TYPE_BLOB
        | oraType::ORA_TYPE_CLOB
        | oraType::ORA_TYPE_NCLOB
        | oraType::ORA_TYPE_BFILE => {
            let data = match value {
                Value::Lob(LobValue::Inline(data)) => data.to_vec(),
                Value::Lob(LobValue::Locator(locator)) => {
                    match block_on(async { state.conn.read_lob(locator).await }) {
                        Ok(LobData::String(text)) => text.into_bytes(),
                        Ok(LobData::Bytes(bytes)) => bytes.to_vec(),
                        Err(err) => raise_pg_error(
                            oraError::FDW_UNABLE_TO_CREATE_REPLY,
                            "error fetching Oracle LOB",
                            err,
                        ),
                    }
                }
                Value::Lob(LobValue::Empty) | Value::Lob(LobValue::Null) => Vec::new(),
                Value::Bytes(bytes) => bytes.clone(),
                Value::String(text) => text.as_bytes().to_vec(),
                other => other.to_string().into_bytes(),
            };

            let handle = Box::into_raw(Box::new(RustLobContent { data }));
            state.lob_handles.push(handle);
            let slot_ptr = ((*column).val as *mut *mut c_void).add(slot);
            *slot_ptr = handle as *mut c_void;

            if !val_len.is_null() {
                *val_len.add(slot) = mem::size_of::<*mut c_void>() as u16;
            }
        }
        _ => {
            let text = match value {
                Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            let max_len = (*column).val_size.max(0) as usize;
            if max_len == 0 || (*column).val.is_null() {
                return;
            }

            let dest = (*column).val.add(slot * max_len);
            let copy_len = text.len().min(max_len.saturating_sub(1));
            ptr::copy_nonoverlapping(text.as_ptr() as *const c_char, dest, copy_len);
            *dest.add(copy_len) = 0;

            if !val_len.is_null() {
                *val_len.add(slot) = copy_len as u16;
            }
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
    unsafe {
        set_output_int(major, values[0]);
        set_output_int(minor, values[1]);
        set_output_int(update, values[2]);
        set_output_int(patch, values[3]);
        set_output_int(port_patch, values[4]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn oracle_rs_register_pg_callbacks(
    callbacks: *const OracleFdwPgCallbacks,
) -> c_int {
    if callbacks.is_null() {
        return -1;
    }

    let callbacks = *callbacks;
    if callbacks.version != ORACLE_FDW_PG_CALLBACKS_VERSION {
        return -2;
    }

    if !has_required_core_callbacks(&callbacks) {
        return -3;
    }

    match pg_callbacks_registry().lock() {
        Ok(mut stored_callbacks) => {
            *stored_callbacks = Some(callbacks);
            0
        }
        Err(_) => -4,
    }
}

/*
 * Reference oracle_utils.c: oracleGetSession
 *
 * C keeps OCI environment/server/user-session handles in envlist, calls
 * initializePostGIS(), starts a remote transaction, sets savepoints to
 * curlevel, and returns a freshly allocated oracleSession pointing at cached
 * connection entries.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleGetSession(
    connectstring: *const c_char,
    isolation_level: oraIsoLevel,
    user: *mut c_char,
    password: *mut c_char,
    _nls_lang: *const c_char,
    _timezone: *const c_char,
    have_nchar: c_int,
    tablename: *const c_char,
    curlevel: c_int,
) -> *mut oracleSession {
    if !pg_callbacks_registered() {
        eprintln!("oracle-rs backend used before oracle_rs_register_pg_callbacks");
        std::process::abort();
    }

    pg_initialize_postgis();
    if let Some(set_handlers) = pg_callbacks().and_then(|callbacks| callbacks.core.set_handlers) {
        set_handlers();
    }

    let connectstring = normalize_connect_string(&cstr_to_string(connectstring));
    let user = cstr_to_string(user);
    let password = cstr_to_string(password);
    let tablename_text = cstr_to_string(tablename);
    let readonly = matches!(isolation_level, oraIsoLevel::ORA_TRANS_READ_ONLY);

    let connect_result = block_on(async {
        let mut config: Config = connectstring.parse()?;
        config.username = user.clone();
        config.set_password(password.clone());
        Connection::connect_with_config(config).await
    });

    let conn = match connect_result {
        Ok(conn) => conn,
        Err(err) => {
            let message = if tablename_text.is_empty() {
                "cannot connect to foreign Oracle server".to_string()
            } else {
                format!(
                    "connection for foreign table \"{}\" cannot be established",
                    tablename_text
                )
            };
            raise_pg_error(oraError::FDW_UNABLE_TO_ESTABLISH_CONNECTION, &message, err);
        }
    };

    let server_version = block_on(async { query_server_version(&conn).await });
    if curlevel > 1 {
        for level in 2..=curlevel {
            let savepoint = format!("s{}", level);
            if let Err(err) = block_on(async { conn.savepoint(&savepoint).await }) {
                raise_pg_error(
                    oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
                    "error setting Oracle savepoint",
                    err,
                );
            }
        }
    }

    let mut state = Box::new(RustSessionState {
        conn,
        current_sql: None,
        current_table: ptr::null(),
        current_result: None,
        next_row: 0,
        import_rows: None,
        import_next: 0,
        lob_handles: Vec::new(),
        xact_level: 1,
        readonly,
    });

    let state_ptr = &mut *state as *mut RustSessionState;
    let mut session = Box::new(oracleSession {
        thin_conn: state_ptr as *mut c_void,
        thin_stmt: ptr::null_mut(),
        thin_result: ptr::null_mut(),
        thin_runtime: Box::into_raw(state) as *mut c_void,
        user_data: ptr::null_mut(),
        have_nchar,
        server_version,
        last_batch: 0,
        fetched_rows: 0,
        current_row: 0,
    });

    let session_ptr = &mut *session as *mut oracleSession;
    let raw_session = Box::into_raw(session);
    register_session(raw_session);

    if let Some(register_callback) =
        pg_callbacks().and_then(|callbacks| callbacks.core.register_callback)
    {
        register_callback(session_ptr as *mut c_void);
    }

    pg_debug2("oracle_fdw: begin remote transaction through oracle-rs");
    raw_session
}

/*
 * Reference oracle_utils.c: oracleCloseStatement
 *
 * C frees OCIStmt and all LOB locators registered below that statement.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleCloseStatement(session: *mut oracleSession) {
    close_statement_state(session);
}

/*
 * Reference oracle_utils.c: oracleCloseConnections
 *
 * C walks envlist -> srvlist -> connlist and closes each cached OCI handle.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleCloseConnections() {
    clear_sessions();
}

/*
 * Reference oracle_utils.c: oracleShutdown
 *
 * C suppresses shutdown errors, closes connections and calls OCITerminate().
 */
#[no_mangle]
pub unsafe extern "C" fn oracleShutdown() {
    clear_sessions();
}

/*
 * Reference oracle_utils.c: oracleCancel
 *
 * C sends OCIBreak to all cached server handles.  oracle-rs currently exposes
 * no direct break packet API, so this is a no-op until protocol cancel support
 * is available.
 */
#[no_mangle]
pub extern "C" fn oracleCancel() {}

/*
 * Reference oracle_utils.c: oracleEndTransaction
 *
 * C receives the registered connection entry, frees statements, then commits
 * or rolls back with OCITransCommit/OCITransRollback.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleEndTransaction(arg: *mut c_void, is_commit: c_int, silent: c_int) {
    if arg.is_null() {
        return;
    }

    let session = arg as *mut oracleSession;
    let state = state_mut(session);
    close_statement_state(session);

    if state.readonly || state.xact_level <= 0 {
        return;
    }

    let result = if is_commit != 0 {
        block_on(async { state.conn.commit().await })
    } else {
        block_on(async { state.conn.rollback().await })
    };

    if let Err(err) = result {
        if silent == 0 {
            raise_pg_error(
                oraError::FDW_UNABLE_TO_CREATE_REPLY,
                "error ending Oracle transaction through oracle-rs",
                err,
            );
        }
    }

    state.xact_level = 0;
}

/*
 * Reference oracle_utils.c: oracleEndSubtransaction
 *
 * C commits by releasing no Oracle resource, or rolls back to savepoint sN.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleEndSubtransaction(
    arg: *mut c_void,
    nest_level: c_int,
    is_commit: c_int,
) {
    if arg.is_null() || is_commit != 0 || nest_level <= 1 {
        return;
    }

    let session = arg as *mut oracleSession;
    let state = state_mut(session);
    let savepoint = format!("s{}", nest_level);
    if let Err(err) = block_on(async { state.conn.rollback_to_savepoint(&savepoint).await }) {
        raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_REPLY,
            "error rolling back Oracle subtransaction through oracle-rs",
            err,
        );
    }
}

/*
 * Reference oracle_utils.c: oracleIsStatementOpen
 *
 * C checks whether session->stmthp is NULL.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleIsStatementOpen(session: *mut oracleSession) -> c_int {
    if session.is_null() {
        return 0;
    }
    (!(*session).thin_stmt.is_null()) as c_int
}

/*
 * Reference oracle_utils.c: oracleDescribe
 *
 * C prepares the user query with WHERE (1=0), reads OCI describe attributes,
 * maps Oracle types to oraType and allocates oraTable/oraColumn through
 * oracleAlloc().
 */
#[no_mangle]
pub unsafe extern "C" fn oracleDescribe(
    session: *mut oracleSession,
    _dblink: *mut c_char,
    schema: *mut c_char,
    table: *mut c_char,
    pgname: *mut c_char,
    _max_long: c_long,
    has_geometry: *mut c_int,
) -> *mut oraTable {
    set_output_int(has_geometry, 0);
    let state = state_mut(session);
    let schema = cstr_to_string(schema);
    let table_name = cstr_to_string(table);
    let pgname = cstr_to_string(pgname);

    let owner_clause = if schema.is_empty() {
        "SYS_CONTEXT('USERENV','CURRENT_SCHEMA')".to_string()
    } else {
        sql_literal(&schema)
    };
    let sql = format!(
        "SELECT column_name, data_type, data_length, data_precision, data_scale, nullable, data_type_owner \
         FROM all_tab_cols \
         WHERE owner = {} AND table_name = {} AND hidden_column = 'NO' \
         ORDER BY column_id",
        owner_clause,
        sql_literal(&table_name)
    );

    let result = match block_on(async { state.conn.query(&sql, &[]).await }) {
        Ok(result) => result,
        Err(err) => raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_REPLY,
            "error describing Oracle table through oracle-rs",
            err,
        ),
    };

    if result.rows.is_empty() {
        raise_pg_error(
            oraError::FDW_TABLE_NOT_FOUND,
            "Oracle table was not found",
            table_name,
        );
    }

    let table_ptr = alloc_zeroed::<oraTable>();
    let cols_ptr = alloc_array::<*mut oraColumn>(result.rows.len());

    (*table_ptr).name = alloc_c_string(&quote_ident(&table_name));
    (*table_ptr).pgname = alloc_c_string(if pgname.is_empty() {
        &table_name
    } else {
        &pgname
    });
    (*table_ptr).ncols = result.rows.len() as c_int;
    (*table_ptr).npgcols = result.rows.len() as c_int;
    (*table_ptr).cols = cols_ptr;

    for (idx, row) in result.rows.iter().enumerate() {
        let colname = value_as_string(row, 0).unwrap_or_default();
        let typename = value_as_string(row, 1).unwrap_or_default();
        let data_length = value_as_i32(row, 2).unwrap_or(0);
        let precision = value_as_i32(row, 3).unwrap_or(0);
        let scale = value_as_i32(row, 4).unwrap_or(0);
        let typeowner = value_as_string(row, 6);
        let oratype = ora_type_from_names(&typename, typeowner.as_deref());

        if oratype == oraType::ORA_TYPE_GEOMETRY {
            set_output_int(has_geometry, 1);
        }

        let column = alloc_zeroed::<oraColumn>();
        (*column).name = alloc_c_string(&quote_ident(&colname));
        (*column).pgname = alloc_c_string(&colname.to_ascii_lowercase());
        (*column).oratype = oratype;
        (*column).scale = scale;
        (*column).pgattnum = (idx + 1) as c_int;
        (*column).pgtype = 0;
        (*column).pgtypmod = -1;
        (*column).used = 1;
        (*column).strip_zeros = 0;
        (*column).pkey = 0;
        (*column).val = ptr::null_mut();
        (*column).val_size = match oratype {
            oraType::ORA_TYPE_NUMBER => 64,
            oraType::ORA_TYPE_DATE => 32,
            oraType::ORA_TYPE_TIMESTAMP
            | oraType::ORA_TYPE_TIMESTAMPTZ
            | oraType::ORA_TYPE_TIMESTAMPLTZ => 64,
            oraType::ORA_TYPE_RAW | oraType::ORA_TYPE_LONGRAW => data_length.max(1),
            oraType::ORA_TYPE_BLOB
            | oraType::ORA_TYPE_CLOB
            | oraType::ORA_TYPE_NCLOB
            | oraType::ORA_TYPE_BFILE => mem::size_of::<*mut c_void>() as i32,
            _ => data_length.max(1) + 1,
        };
        (*column).val_len = ptr::null_mut();
        (*column).val_null = ptr::null_mut();
        (*column).val_len4 = 0;
        (*column).varno = 0;

        *cols_ptr.add(idx) = column;

        let _ = precision;
    }

    table_ptr
}

/*
 * Reference oracle_utils.c: oracleExplain
 *
 * C runs EXPLAIN PLAN, queries PLAN_TABLE with tree indentation, copies each
 * line into oracleAlloc() memory and returns nrows/plan.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleExplain(
    session: *mut oracleSession,
    query: *const c_char,
    nrows: *mut c_int,
    plan: *mut *mut *mut c_char,
) {
    set_output_int(nrows, 0);
    set_output_ptr(plan, ptr::null_mut());

    let state = state_mut(session);
    let query = cstr_to_string(query);
    let statement_id = format!("ORACLE_FDW_RS_{}", std::process::id());
    let explain_sql = format!(
        "EXPLAIN PLAN SET STATEMENT_ID = {} FOR {}",
        sql_literal(&statement_id),
        query
    );
    if let Err(err) = block_on(async { state.conn.execute(&explain_sql, &[]).await }) {
        raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
            "error explaining Oracle query through oracle-rs",
            err,
        );
    }

    let plan_sql = format!(
        "SELECT LPAD(' ', 2 * LEVEL - 2) || operation || \
         CASE WHEN options IS NULL THEN '' ELSE ' ' || options END || \
         CASE WHEN object_name IS NULL THEN '' ELSE ' ' || object_name END AS plan_line \
         FROM plan_table \
         WHERE statement_id = {} \
         START WITH id = 0 \
         CONNECT BY PRIOR id = parent_id AND statement_id = {} \
         ORDER SIBLINGS BY position",
        sql_literal(&statement_id),
        sql_literal(&statement_id)
    );
    let result = match block_on(async { state.conn.query(&plan_sql, &[]).await }) {
        Ok(result) => result,
        Err(err) => raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_REPLY,
            "error reading Oracle query plan through oracle-rs",
            err,
        ),
    };

    let _ = block_on(async {
        state
            .conn
            .execute(
                &format!(
                    "DELETE FROM plan_table WHERE statement_id = {}",
                    sql_literal(&statement_id)
                ),
                &[],
            )
            .await
    });

    let plan_ptr = alloc_array::<*mut c_char>(result.rows.len());
    for (idx, row) in result.rows.iter().enumerate() {
        let line = value_as_string(row, 0).unwrap_or_default();
        *plan_ptr.add(idx) = alloc_c_string(&line);
    }

    set_output_int(nrows, result.rows.len() as c_int);
    set_output_ptr(plan, plan_ptr);
}

/*
 * Reference oracle_utils.c: oraclePrepareQuery
 *
 * C obtains an OCIStmt handle, configures prefetch/LOB prefetch and defines
 * output buffers against oraTable columns.
 */
#[no_mangle]
pub unsafe extern "C" fn oraclePrepareQuery(
    session: *mut oracleSession,
    query: *const c_char,
    oraTable: *const oraTable,
    _prefetch: c_uint,
    _lob_prefetch: c_uint,
) {
    close_statement_state(session);
    let state = state_mut(session);
    state.current_sql = Some(cstr_to_string(query));
    state.current_table = oraTable;
    (*session).thin_stmt = state.current_sql.as_ref().unwrap().as_ptr() as *mut c_void;
}

/*
 * Reference oracle_utils.c: oracleExecuteQuery
 *
 * C binds paramDesc values, executes OCIStmtExecute and records the number of
 * rows in the first fetch batch.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleExecuteQuery(
    session: *mut oracleSession,
    _oraTable: *const oraTable,
    _paramList: *mut paramDesc,
    _prefetch: c_uint,
) -> c_uint {
    let state = state_mut(session);
    let Some(sql) = state.current_sql.clone() else {
        return 0;
    };

    let result = match block_on(async { state.conn.query(&sql, &[]).await }) {
        Ok(result) => result,
        Err(Error::Protocol(_)) => match block_on(async { state.conn.execute(&sql, &[]).await }) {
            Ok(result) => result,
            Err(err) => raise_pg_error(
                oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
                "error executing Oracle query through oracle-rs",
                err,
            ),
        },
        Err(err) => raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
            "error executing Oracle query through oracle-rs",
            err,
        ),
    };

    let processed = if result.rows.is_empty() {
        result.rows_affected as c_uint
    } else {
        result.rows.len() as c_uint
    };
    state.current_result = Some(result);
    state.next_row = 0;
    (*session).thin_result =
        state.current_result.as_mut().unwrap() as *mut QueryResult as *mut c_void;
    (*session).fetched_rows = processed;
    (*session).current_row = 0;
    processed
}

/*
 * Reference oracle_utils.c: oracleFetchNext
 *
 * C fetches the next OCI row/batch, fills each used oraColumn buffer and
 * returns a 1-based row index within the current prefetch batch.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleFetchNext(session: *mut oracleSession, prefetch: c_uint) -> c_uint {
    let state = state_mut(session);
    let Some(result) = state.current_result.as_ref() else {
        return 0;
    };
    if state.next_row >= result.rows.len() {
        (*session).last_batch = 1;
        return 0;
    }

    let table = state.current_table;
    if table.is_null() {
        return 0;
    }

    let slot = if prefetch == 0 {
        0
    } else {
        state.next_row % prefetch as usize
    };
    let row = &result.rows[state.next_row];

    for idx in 0..(*table).ncols.max(0) as usize {
        let column = *(*table).cols.add(idx);
        if column.is_null() || (*column).used == 0 {
            continue;
        }
        let value = row.get(idx).unwrap_or(&Value::Null);
        copy_value_to_column(session, column, slot, value);
    }

    state.next_row += 1;
    (*session).current_row = (slot + 1) as c_uint;
    (*session).last_batch = (state.next_row >= result.rows.len()) as c_uint;
    (*session).current_row
}

/*
 * Reference oracle_utils.c: oracleExecuteCall
 *
 * C prepares a statement, executes it once, then closes the statement.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleExecuteCall(session: *mut oracleSession, stmt: *mut c_char) {
    let state = state_mut(session);
    let sql = cstr_to_string(stmt);
    if let Err(err) = block_on(async { state.conn.execute(&sql, &[]).await }) {
        raise_pg_error(
            oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
            "error executing Oracle statement through oracle-rs",
            err,
        );
    }
}

/*
 * Reference oracle_utils.c: oracleGetLob
 *
 * C reads OCILobLocator in LOB_CHUNK_SIZE chunks, reallocating the output
 * buffer with oracleRealloc().
 */
#[no_mangle]
pub unsafe extern "C" fn oracleGetLob(
    _session: *mut oracleSession,
    locptr: *mut c_void,
    _type: oraType,
    value: *mut *mut c_char,
    value_len: *mut c_long,
) {
    set_output_ptr(value, ptr::null_mut());
    set_output_long(value_len, 0);

    if locptr.is_null() {
        return;
    }

    let handle = *(locptr as *mut *mut RustLobContent);
    if handle.is_null() {
        return;
    }

    let data = &(*handle).data;
    let ptr = pg_alloc(data.len() + 1) as *mut c_char;
    if ptr.is_null() {
        raise_pg_error(
            oraError::FDW_OUT_OF_MEMORY,
            "oracle-rs failed to allocate LOB memory",
            data.len() + 1,
        );
    }
    ptr::copy_nonoverlapping(data.as_ptr() as *const c_char, ptr, data.len());
    *ptr.add(data.len()) = 0;

    set_output_ptr(value, ptr);
    set_output_long(value_len, data.len() as c_long);
}

/*
 * Reference oracle_utils.c: oracleClientVersion
 *
 * C calls OCIClientVersion().
 */
#[no_mangle]
pub extern "C" fn oracleClientVersion(
    major: *mut c_int,
    minor: *mut c_int,
    update: *mut c_int,
    patch: *mut c_int,
    port_patch: *mut c_int,
) {
    set_version_fields(
        major,
        minor,
        update,
        patch,
        port_patch,
        ORACLE_RS_CLIENT_VERSION,
    );
}

/*
 * Reference oracle_utils.c: oracleServerVersion
 *
 * C returns the version cached in srvEntry during login.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleServerVersion(
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

    set_version_fields(
        major,
        minor,
        update,
        patch,
        port_patch,
        (*session).server_version,
    );
}

/*
 * Reference oracle_utils.c: oracleGetGeometryType
 *
 * C lazily resolves/caches MDSYS.SDO_GEOMETRY through OCITypeByName().
 */
#[no_mangle]
pub extern "C" fn oracleGetGeometryType(_session: *mut oracleSession) -> *mut c_void {
    ptr::null_mut()
}

fn build_import_query(
    schema: &str,
    limit_to: &str,
    skip_tables: c_int,
    skip_views: c_int,
    skip_matviews: c_int,
) -> String {
    let owner_clause = if schema.is_empty() {
        "SYS_CONTEXT('USERENV','CURRENT_SCHEMA')".to_string()
    } else {
        sql_literal(schema)
    };

    let mut object_filters = Vec::new();
    if skip_tables == 0 {
        object_filters.push("'TABLE'");
    }
    if skip_views == 0 {
        object_filters.push("'VIEW'");
    }
    if skip_matviews == 0 {
        object_filters.push("'MATERIALIZED VIEW'");
    }
    if object_filters.is_empty() {
        object_filters.push("'TABLE'");
    }

    let table_filter = if limit_to.trim().is_empty() {
        String::new()
    } else {
        format!("AND c.table_name IN ({})", limit_to)
    };

    format!(
        "SELECT c.table_name, c.column_name, c.data_type, c.data_type_owner, \
                NVL(c.char_col_decl_length, 0), NVL(c.data_precision, 0), NVL(c.data_scale, 0), \
                c.nullable, CASE WHEN pk.column_name IS NULL THEN 0 ELSE 1 END \
         FROM all_tab_cols c \
         JOIN all_objects o ON o.owner = c.owner AND o.object_name = c.table_name \
         LEFT JOIN ( \
           SELECT acc.owner, acc.table_name, acc.column_name \
           FROM all_constraints ac \
           JOIN all_cons_columns acc ON acc.owner = ac.owner \
             AND acc.constraint_name = ac.constraint_name \
             AND acc.table_name = ac.table_name \
           WHERE ac.constraint_type = 'P' \
         ) pk ON pk.owner = c.owner AND pk.table_name = c.table_name AND pk.column_name = c.column_name \
         WHERE c.owner = {} \
           AND c.hidden_column = 'NO' \
           AND o.object_type IN ({}) \
           {} \
         ORDER BY c.table_name, c.column_id",
        owner_clause,
        object_filters.join(", "),
        table_filter
    )
}

/*
 * Reference oracle_utils.c: oracleGetImportColumn
 *
 * C opens a metadata query on the first call, defines nine output columns,
 * returns one column per call and closes the statement at OCI_NO_DATA.
 */
#[no_mangle]
pub unsafe extern "C" fn oracleGetImportColumn(
    session: *mut oracleSession,
    _dblink: *mut c_char,
    schema: *mut c_char,
    limit_to: *mut c_char,
    tabname: *mut *mut c_char,
    colname: *mut *mut c_char,
    type_out: *mut oraType,
    charlen: *mut c_int,
    typeprec: *mut c_int,
    typescale: *mut c_int,
    nullable: *mut c_int,
    key: *mut c_int,
    skip_tables: c_int,
    skip_views: c_int,
    skip_matviews: c_int,
) -> c_int {
    set_output_ptr(tabname, ptr::null_mut());
    set_output_ptr(colname, ptr::null_mut());

    let state = state_mut(session);
    if state.import_rows.is_none() {
        let schema = cstr_to_string(schema);
        let limit_to = cstr_to_string(limit_to);
        let sql = build_import_query(&schema, &limit_to, skip_tables, skip_views, skip_matviews);
        let result = match block_on(async { state.conn.query(&sql, &[]).await }) {
            Ok(result) => result,
            Err(err) => raise_pg_error(
                oraError::FDW_UNABLE_TO_CREATE_EXECUTION,
                "error importing Oracle schema through oracle-rs",
                err,
            ),
        };

        let rows = result
            .rows
            .iter()
            .map(|row| ImportColumn {
                tabname: value_as_string(row, 0).unwrap_or_default(),
                colname: value_as_string(row, 1).unwrap_or_default(),
                typename: value_as_string(row, 2).unwrap_or_default(),
                typeowner: value_as_string(row, 3),
                charlen: value_as_i32(row, 4).unwrap_or(0),
                typeprec: value_as_i32(row, 5).unwrap_or(0),
                typescale: value_as_i32(row, 6).unwrap_or(0),
                nullable: (value_as_string(row, 7).unwrap_or_default() == "Y") as c_int,
                key: value_as_i32(row, 8).unwrap_or(0),
            })
            .collect::<Vec<_>>();
        state.import_rows = Some(rows);
        state.import_next = 0;
        (*session).thin_stmt = state.import_rows.as_ref().unwrap().as_ptr() as *mut c_void;
    }

    let rows = state.import_rows.as_ref().unwrap();
    if state.import_next >= rows.len() {
        state.import_rows = None;
        state.import_next = 0;
        (*session).thin_stmt = ptr::null_mut();
        return 0;
    }

    let row = &rows[state.import_next];
    state.import_next += 1;

    set_output_ptr(tabname, alloc_c_string(&row.tabname));
    set_output_ptr(colname, alloc_c_string(&row.colname));
    if !type_out.is_null() {
        *type_out = ora_type_from_names(&row.typename, row.typeowner.as_deref());
    }
    set_output_int(charlen, row.charlen);
    set_output_int(typeprec, row.typeprec);
    set_output_int(typescale, row.typescale);
    set_output_int(nullable, row.nullable);
    set_output_int(key, row.key);

    1
}
