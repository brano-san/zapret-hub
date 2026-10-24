/// Local wall-clock time: log file names, test run ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

impl LocalTime {
    #[cfg(windows)]
    pub fn now() -> Self {
        // SAFETY: GetLocalTime has no preconditions.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        Self {
            year: t.wYear,
            month: t.wMonth,
            day: t.wDay,
            hour: t.wHour,
            minute: t.wMinute,
            second: t.wSecond,
        }
    }

    #[cfg(unix)]
    pub fn now() -> Self {
        // SAFETY: `time` with a null pointer only returns; `localtime_r` fills the zeroed `tm`
        // we own (an all-zero `tm` is valid, its zone pointer is just null).
        let t = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut t: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut t);
            t
        };
        let field = |v: libc::c_int| u16::try_from(v).unwrap_or_default();
        Self {
            year: field(t.tm_year + 1900),
            month: field(t.tm_mon + 1),
            day: field(t.tm_mday),
            hour: field(t.tm_hour),
            minute: field(t.tm_min),
            second: field(t.tm_sec),
        }
    }

    /// `2026-10-10`
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `2026-10-10_05-03-34`: how `test zapret.ps1` names its results.
    pub fn stamp(&self) -> String {
        format!(
            "{}_{:02}-{:02}-{:02}",
            self.date(),
            self.hour,
            self.minute,
            self.second
        )
    }

    /// `05:03:34`
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}
