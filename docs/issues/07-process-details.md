# Process details: services of svchost, user, command line, icon

## Summary
Many rows belong to `svchost.exe`, which says nothing. Show which service owns the endpoint and more about the process.

## Details
- **Service** column: services hosted in the PID (`EnumServicesStatusExW` with `SERVICE_STATUS_PROCESS.dwProcessId`), or better the exact service of the endpoint via the owner module tables (`TCP_TABLE_OWNER_MODULE_ALL` + `GetOwnerModuleFromTcpEntry`), see the create-time issue.
- **User** column (process token owner), **Command line** (via `NtQueryInformationProcess` / WMI) and **Company / description** from the version resource – shown in a details pane at the bottom (like the details pane of RusTrace) and as tooltip.
- Process icon in the Process column (`SHGetFileInfoW`, cached per path).
- All lookups cached per PID + process start time (PID reuse!).

## Acceptance
- Details pane shows data for the selected row; columns can be switched on in View → Columns.
