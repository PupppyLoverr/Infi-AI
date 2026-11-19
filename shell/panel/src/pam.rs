//! Password check through PAM, the system's own authentication stack (the
//! same path `login` and `swaylock` use). A small direct binding: three
//! functions and a conversation callback that answers the password prompt.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        handle: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}

extern "C" {
    fn calloc(n: usize, size: usize) -> *mut c_void;
    fn strdup(s: *const c_char) -> *mut c_char;
}

/// Answers every hidden prompt with the password, visible prompts with the
/// user name, and ignores informational messages.
extern "C" fn converse(
    n: c_int,
    msgs: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if n <= 0 || msgs.is_null() || resp.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: PAM passes `n` messages; `data` is the (user, password) pair we
    // gave pam_start, alive for the whole call. Replies are malloc'd because
    // PAM frees them.
    unsafe {
        let creds = &*(data as *const (CString, CString));
        let replies = calloc(n as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if replies.is_null() {
            return PAM_BUF_ERR;
        }
        for i in 0..n as usize {
            // Linux-PAM passes an array of pointers.
            let m = *msgs.add(i);
            let style = if m.is_null() { 0 } else { (*m).msg_style };
            let r = &mut *replies.add(i);
            r.resp_retcode = 0;
            r.resp = match style {
                PAM_PROMPT_ECHO_OFF => strdup(creds.1.as_ptr()),
                PAM_PROMPT_ECHO_ON => strdup(creds.0.as_ptr()),
                _ => ptr::null_mut(),
            };
        }
        *resp = replies;
    }
    PAM_SUCCESS
}

/// Check `password` for `user` against PAM service `service`.
pub fn authenticate(service: &str, user: &str, password: &str) -> Result<(), String> {
    let service = CString::new(service).map_err(|_| "bad service name")?;
    let creds = Box::new((
        CString::new(user).map_err(|_| "bad user name")?,
        CString::new(password).map_err(|_| "the password contains a NUL")?,
    ));
    let conv = PamConv {
        conv: converse,
        appdata_ptr: &*creds as *const _ as *mut c_void,
    };
    let mut handle: *mut c_void = ptr::null_mut();
    // SAFETY: valid C strings and a conversation struct that outlive the handle.
    unsafe {
        let rc = pam_start(service.as_ptr(), creds.0.as_ptr(), &conv, &mut handle);
        if rc != PAM_SUCCESS || handle.is_null() {
            return Err(format!("PAM could not start (code {rc})"));
        }
        let mut rc = pam_authenticate(handle, 0);
        if rc == PAM_SUCCESS {
            rc = pam_acct_mgmt(handle, 0);
        }
        pam_end(handle, rc);
        if rc == PAM_SUCCESS {
            Ok(())
        } else {
            Err("Wrong password".into())
        }
    }
}

/// The logged-in user's name.
pub fn current_user() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| {
            // SAFETY: getlogin returns a static string or null.
            unsafe {
                let p = libc_getlogin();
                if p.is_null() {
                    "user".into()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            }
        })
}

extern "C" {
    #[link_name = "getlogin"]
    fn libc_getlogin() -> *const c_char;
}

/// The user's full name from `/etc/passwd` (GECOS), else the user name.
pub fn full_name(user: &str, passwd: &str) -> String {
    passwd
        .lines()
        .find(|l| l.split(':').next() == Some(user))
        .and_then(|l| l.split(':').nth(4))
        .map(|g| g.split(',').next().unwrap_or("").trim().to_string())
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| user.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gecos_full_name() {
        let pw = "root:x:0:0:root:/root:/bin/bash\nada:x:1000:1000:Ada Lovelace,,,:/home/ada:/bin/bash\nbob:x:1001:1001::/home/bob:/bin/sh\n";
        assert_eq!(full_name("ada", pw), "Ada Lovelace");
        assert_eq!(full_name("bob", pw), "bob");
        assert_eq!(full_name("zed", pw), "zed");
    }

    /// Run by hand against a real account:
    /// `SOLYRA_PAM_USER=u SOLYRA_PAM_PASS=p cargo test pam_real -- --ignored`
    #[test]
    #[ignore]
    fn pam_real_account() {
        let user = std::env::var("SOLYRA_PAM_USER").unwrap();
        let pass = std::env::var("SOLYRA_PAM_PASS").unwrap();
        let service = std::env::var("SOLYRA_PAM_SERVICE").unwrap_or_else(|_| "solyra-lock".into());
        assert!(
            authenticate(&service, &user, &pass).is_ok(),
            "right password refused"
        );
        assert!(
            authenticate(&service, &user, "not-the-password").is_err(),
            "wrong password accepted"
        );
    }

    #[test]
    fn a_wrong_password_is_refused() {
        // `other` (deny by default on most systems) or a missing service:
        // either way this must never succeed.
        assert!(
            authenticate("solyra-lock-test-nonexistent", "nobody", "definitely-wrong").is_err()
        );
    }
}
