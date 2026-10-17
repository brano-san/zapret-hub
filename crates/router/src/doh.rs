//! Whether Windows itself resolves names over DNS over HTTPS: then the DNS sniffer sees no answers
//! and domain rules match nothing. Read-only registry lookups.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_DWORD, RRF_RT_QWORD, RegCloseKey, RegEnumKeyExW,
    RegGetValueW, RegOpenKeyExW,
};
use windows::core::{HSTRING, PWSTR};

const INTERFACES: &str = r"SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters";
const PARAMETERS: &str = r"SYSTEM\CurrentControlSet\Services\Dnscache\Parameters";
const POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows NT\DNSClient";

/// True when an adapter or a Wi-Fi profile has encrypted DNS on (`DohFlags` != 0 under
/// `...\{adapter}\Doh{Interface,Profile}Settings\Doh{,6}\{server}`), the policy requires it
/// (`DoHPolicy` = 3), or automatic upgrade is on (`EnableAutoDoh` = 2).
// ponytail: settings of adapters that are not connected count too; check only connected ones if
// that misleads.
pub fn system_doh() -> bool {
    value(POLICY, "DoHPolicy") == Some(3)
        || value(PARAMETERS, "EnableAutoDoh") == Some(2)
        || open(HKEY_LOCAL_MACHINE, INTERFACES).is_some_and(|k| any_flags(&k, 4))
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(parent: HKEY, path: &str) -> Option<Key> {
    let mut key = HKEY::default();
    let path = HSTRING::from(path);
    (unsafe { RegOpenKeyExW(parent, &path, None, KEY_READ, &mut key) } == ERROR_SUCCESS)
        .then_some(Key(key))
}

/// A DWORD or QWORD value; `None` if it is missing or of another type.
fn read(key: HKEY, sub: &str, name: &str) -> Option<u64> {
    let mut data = 0u64;
    let mut size = size_of::<u64>() as u32;
    let (sub, name) = (HSTRING::from(sub), HSTRING::from(name));
    let ok = unsafe {
        RegGetValueW(
            key,
            &sub,
            &name,
            RRF_RT_DWORD | RRF_RT_QWORD,
            None,
            Some(&mut data as *mut u64 as *mut _),
            Some(&mut size),
        )
    } == ERROR_SUCCESS;
    ok.then_some(data)
}

fn value(path: &str, name: &str) -> Option<u64> {
    read(HKEY_LOCAL_MACHINE, path, name)
}

/// Any `DohFlags` != 0 in `key` or its subkeys down to `depth` levels.
fn any_flags(key: &Key, depth: u32) -> bool {
    if read(key.0, "", "DohFlags").is_some_and(|f| f != 0) {
        return true;
    }
    if depth == 0 {
        return false;
    }
    let mut name = [0u16; 256];
    (0..)
        .map_while(|i| {
            let mut len = name.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(
                    key.0,
                    i,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            (r == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&name[..len as usize]))
        })
        .collect::<Vec<_>>()
        .iter()
        .any(|sub| open(key.0, sub).is_some_and(|k| any_flags(&k, depth - 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_machine_without_failing() {
        // The answer depends on the machine; the walk must finish and a missing key reads as off.
        let _ = system_doh();
        assert_eq!(value(r"SOFTWARE\zapret-hub-no-such-key", "DohFlags"), None);
    }
}
