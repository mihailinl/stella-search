//! Windows-specific platform implementation

use anyhow::Result;

#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
const SERVICE_NAME: &str = "StellaSearch";
#[cfg(windows)]
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

/// Run as a Windows service
#[cfg(windows)]
pub fn run_service() -> Result<()> {
    // Register the service
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, service_main);

#[cfg(windows)]
fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service_main() {
        tracing::error!("Service failed: {}", e);
    }
}

#[cfg(windows)]
fn run_service_main() -> Result<()> {
    use tokio::runtime::Runtime;

    // Create event handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                // Signal shutdown
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register the service control handler
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    // Tell Windows we're starting
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    })?;

    // Create async runtime
    let rt = Runtime::new()?;

    // Tell Windows we're running
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Run the daemon
    rt.block_on(async {
        crate::run_daemon().await
    })?;

    // Tell Windows we've stopped
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

/// Get all available drive letters on Windows
#[cfg(windows)]
pub fn get_drive_letters() -> Vec<String> {
    let mut drives = Vec::new();

    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let path = std::path::Path::new(&drive);
        if path.exists() {
            drives.push(drive);
        }
    }

    drives
}

#[cfg(not(windows))]
pub fn run_service() -> Result<()> {
    anyhow::bail!("Windows service mode is only available on Windows")
}

#[cfg(not(windows))]
pub fn get_drive_letters() -> Vec<String> {
    Vec::new()
}
