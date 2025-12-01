//! Windows Search backend using direct COM/OLE DB
//!
//! Queries the Windows Search Index using ADO via COM.
//! No PowerShell, no process spawning, no window flashing.

use stella_search_core::{IndexedFile, SearchResults};
use std::time::Instant;
use windows::{
    core::*,
    Win32::Globalization::LOCALE_USER_DEFAULT,
    Win32::System::Com::*,
    Win32::System::Services::*,
};

// ADODB.Connection CLSID (not in windows-rs, define manually)
const CLSID_ADODB_CONNECTION: GUID = GUID::from_u128(0x00000514_0000_0010_8000_00aa006d2ea4);

/// Check if Windows Search service is running
pub fn is_available() -> bool {
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT);
        if scm.is_err() {
            return false;
        }
        let scm = scm.unwrap();

        let service_name = w!("WSearch");
        let service = OpenServiceW(scm, service_name, SERVICE_QUERY_STATUS);
        if service.is_err() {
            let _ = CloseServiceHandle(scm);
            return false;
        }
        let service = service.unwrap();

        let mut status = SERVICE_STATUS::default();
        let running =
            QueryServiceStatus(service, &mut status).is_ok() && status.dwCurrentState == SERVICE_RUNNING;

        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);
        running
    }
}

/// Search for files using Windows Search via direct COM
pub fn search(
    query: &str,
    max_results: u32,
    extension: Option<&str>,
) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();

    let files = unsafe { search_via_com(query, max_results, extension)? };

    let search_results = SearchResults {
        files: files.clone(),
        total_found: files.len(),
        query_time_ms: start.elapsed().as_millis() as u64,
    };

    Ok(serde_json::to_string(&search_results)?)
}

/// Execute the COM-based search
unsafe fn search_via_com(
    query: &str,
    max_results: u32,
    extension: Option<&str>,
) -> std::result::Result<Vec<IndexedFile>, Box<dyn std::error::Error + Send + Sync>> {
    // Initialize COM (apartment-threaded for ADO)
    let _com = ComInitializer::new()?;

    // Create ADODB.Connection
    let conn: IDispatch = unsafe { CoCreateInstance(&CLSID_ADODB_CONNECTION, None, CLSCTX_INPROC_SERVER)? };

    // Open connection to Windows Search
    let conn_string = "Provider=Search.CollatorDSO;Extended Properties='Application=Windows'";
    unsafe { invoke_method(&conn, "Open", &[VARIANT::from(conn_string)])? };

    // Build and execute SQL query
    let sql = build_search_sql(query, max_results, extension);
    let rs_variant = unsafe { invoke_method(&conn, "Execute", &[VARIANT::from(sql.as_str())])? };

    // Get IDispatch for recordset
    let rs: IDispatch = IDispatch::try_from(&rs_variant)
        .map_err(|e| format!("Failed to get recordset IDispatch: {}", e))?;

    // Read results
    let files = unsafe { read_recordset(&rs)? };

    // Close recordset and connection (ignore errors)
    let _ = unsafe { invoke_method(&rs, "Close", &[]) };
    let _ = unsafe { invoke_method(&conn, "Close", &[]) };

    Ok(files)
}

/// RAII wrapper for COM initialization
struct ComInitializer {
    should_uninit: bool,
}

impl ComInitializer {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            // Try apartment-threaded first (required for ADO)
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            // S_OK (0) = success, S_FALSE (1) = already initialized on this thread
            // RPC_E_CHANGED_MODE = already initialized with different mode
            let should_uninit = hr.is_ok();

            // If already initialized with different mode, that's okay - we can still use COM
            // The error code for "already initialized" is acceptable
            if hr.is_err() {
                let code = hr.0 as u32;
                // 0x80010106 = RPC_E_CHANGED_MODE - already initialized with different threading model
                // This is fine, we can still use COM objects
                if code != 0x80010106 {
                    return Err(format!("CoInitializeEx failed: 0x{:08X}", code).into());
                }
            }

            Ok(ComInitializer { should_uninit })
        }
    }
}

impl Drop for ComInitializer {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Invoke a method on IDispatch by name
unsafe fn invoke_method(
    disp: &IDispatch,
    name: &str,
    args: &[VARIANT],
) -> windows::core::Result<VARIANT> {
    let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut dispid: i32 = 0;
    let mut names = [PCWSTR(name_wide.as_ptr())];

    unsafe {
        disp.GetIDsOfNames(&GUID::zeroed(), names.as_mut_ptr(), 1, LOCALE_USER_DEFAULT, &mut dispid)?;
    }

    let mut result = VARIANT::default();
    let mut exc = EXCEPINFO::default();
    let mut arg_err: u32 = 0;

    // Args in reverse order for DISPPARAMS (COM convention)
    let mut reversed_args: Vec<VARIANT> = args.iter().rev().cloned().collect();

    let params = DISPPARAMS {
        rgvarg: if reversed_args.is_empty() {
            std::ptr::null_mut()
        } else {
            reversed_args.as_mut_ptr()
        },
        rgdispidNamedArgs: std::ptr::null_mut(),
        cArgs: args.len() as u32,
        cNamedArgs: 0,
    };

    unsafe {
        disp.Invoke(
            dispid,
            &GUID::zeroed(),
            LOCALE_USER_DEFAULT,
            DISPATCH_METHOD,
            &params,
            Some(&mut result),
            Some(&mut exc),
            Some(&mut arg_err),
        )?;
    }

    Ok(result)
}

/// Get property value from IDispatch
unsafe fn get_property(disp: &IDispatch, name: &str) -> windows::core::Result<VARIANT> {
    let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut dispid: i32 = 0;
    let mut names = [PCWSTR(name_wide.as_ptr())];

    unsafe {
        disp.GetIDsOfNames(&GUID::zeroed(), names.as_mut_ptr(), 1, LOCALE_USER_DEFAULT, &mut dispid)?;
    }

    let mut result = VARIANT::default();
    let params = DISPPARAMS::default();

    unsafe {
        disp.Invoke(
            dispid,
            &GUID::zeroed(),
            LOCALE_USER_DEFAULT,
            DISPATCH_PROPERTYGET,
            &params,
            Some(&mut result),
            None,
            None,
        )?;
    }

    Ok(result)
}

/// Read recordset rows into IndexedFile vec
unsafe fn read_recordset(
    rs: &IDispatch,
) -> std::result::Result<Vec<IndexedFile>, Box<dyn std::error::Error + Send + Sync>> {
    let mut files = Vec::new();

    loop {
        // Check EOF
        let eof_variant = unsafe { get_property(rs, "EOF")? };
        // Try to convert to bool - if it fails or is true, we're done
        let is_eof = bool::try_from(&eof_variant).unwrap_or(true);
        if is_eof {
            break;
        }

        // Get Fields collection
        let fields_variant = unsafe { get_property(rs, "Fields")? };
        let fields: IDispatch = IDispatch::try_from(&fields_variant)
            .map_err(|e| format!("Failed to get Fields IDispatch: {}", e))?;

        // Get field values
        let path = unsafe { get_field_string(&fields, "System.ItemPathDisplay").unwrap_or_default() };
        let name = unsafe { get_field_string(&fields, "System.FileName").unwrap_or_default() };
        let item_type = unsafe { get_field_string(&fields, "System.ItemType").ok() };
        let size = unsafe { get_field_i64(&fields, "System.Size").unwrap_or(0) };

        // Skip if path is empty
        if !path.is_empty() {
            let is_dir = item_type
                .as_ref()
                .map(|t| t == "Directory" || t == "Folder" || t.is_empty())
                .unwrap_or(true);

            files.push(IndexedFile {
                id: 0,
                path,
                name,
                extension: if is_dir { None } else { item_type },
                size,
                is_directory: is_dir,
            });
        }

        // MoveNext
        unsafe { invoke_method(rs, "MoveNext", &[])? };
    }

    Ok(files)
}

/// Get field value as string
unsafe fn get_field_string(
    fields: &IDispatch,
    field_name: &str,
) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let field_variant = unsafe { invoke_method(fields, "Item", &[VARIANT::from(field_name)])? };
    let field: IDispatch = IDispatch::try_from(&field_variant)
        .map_err(|e| format!("Failed to get field IDispatch: {}", e))?;
    let value = unsafe { get_property(&field, "Value")? };

    // Try to convert to BSTR first
    if let Ok(bstr) = BSTR::try_from(&value) {
        return Ok(bstr.to_string());
    }

    // Check if it's empty/null by trying to see if Display works
    let display = format!("{}", value);
    if display.is_empty() || display == "(null)" || display == "(empty)" {
        return Ok(String::new());
    }

    Ok(display)
}

/// Get field value as i64
unsafe fn get_field_i64(
    fields: &IDispatch,
    field_name: &str,
) -> std::result::Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let field_variant = unsafe { invoke_method(fields, "Item", &[VARIANT::from(field_name)])? };
    let field: IDispatch = IDispatch::try_from(&field_variant)
        .map_err(|e| format!("Failed to get field IDispatch: {}", e))?;
    let value = unsafe { get_property(&field, "Value")? };

    // Try different integer conversions
    if let Ok(val) = i64::try_from(&value) {
        return Ok(val);
    }
    if let Ok(val) = i32::try_from(&value) {
        return Ok(val as i64);
    }
    if let Ok(val) = i16::try_from(&value) {
        return Ok(val as i64);
    }
    if let Ok(val) = u64::try_from(&value) {
        return Ok(val as i64);
    }
    if let Ok(val) = u32::try_from(&value) {
        return Ok(val as i64);
    }
    if let Ok(val) = u16::try_from(&value) {
        return Ok(val as i64);
    }
    if let Ok(val) = f64::try_from(&value) {
        return Ok(val as i64);
    }

    // Default to 0 if we can't convert
    Ok(0)
}

/// Build SQL query for Windows Search SystemIndex
fn build_search_sql(query: &str, max_results: u32, extension: Option<&str>) -> String {
    let mut conditions = Vec::new();
    let escaped_query = query.replace('\'', "''");
    conditions.push(format!("System.FileName LIKE '%{}%'", escaped_query));

    if let Some(ext) = extension {
        let escaped_ext = ext.replace('\'', "''");
        conditions.push(format!("System.ItemType = '{}'", escaped_ext));
    }

    format!(
        "SELECT TOP {} System.ItemPathDisplay, System.FileName, System.ItemType, System.Size \
         FROM SystemIndex WHERE {} ORDER BY System.Search.Rank DESC",
        max_results,
        conditions.join(" AND ")
    )
}
