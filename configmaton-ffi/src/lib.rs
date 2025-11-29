use configmaton::blob::automaton::Automaton;
use configmaton::configmaton::Configmaton;
use configmaton::keyval_nfa::Msg;
use configmaton::onion::ThreadUnsafeLocker;

type MyConfigmaton = Configmaton<'static, ThreadUnsafeLocker>;
pub struct FfiConfigmaton;

pub struct OwnedConfigmaton {
    _msg: Msg,
    configmaton: MyConfigmaton,
}

#[no_mangle]
pub unsafe extern "C" fn new_configmaton_base(
    buf: *const u8,
    len: usize,
) -> *mut OwnedConfigmaton {
    // Re-deserialize to fix up internal pointers
    let msg = Msg::read(|msgbuf| std::ptr::copy_nonoverlapping(buf, msgbuf, len), len);
    let aut = msg.get_automaton() as *const _ as *const Automaton<'static>;
    let configmaton = Configmaton::new(&*aut);

    Box::into_raw(Box::new(OwnedConfigmaton { _msg: msg, configmaton }))
}

#[no_mangle]
pub unsafe extern "C" fn drop_configmaton_base(base: *mut OwnedConfigmaton) {
    drop(Box::from_raw(base));
}

#[no_mangle]
pub unsafe extern "C" fn base_get_configmaton(base: *mut OwnedConfigmaton) -> *mut FfiConfigmaton {
    &mut (*base).configmaton as *mut _ as *mut FfiConfigmaton
}

#[no_mangle]
pub unsafe extern "C" fn configmaton_make_child(
    configmaton: *mut FfiConfigmaton,
) -> *mut FfiConfigmaton {
    let configmaton = &mut *(configmaton as *mut MyConfigmaton);
    configmaton.make_child() as *mut FfiConfigmaton
}

#[no_mangle]
pub unsafe extern "C" fn configmaton_set(
    configmaton: *mut FfiConfigmaton,
    key: *const u8,
    key_len: usize,
    value: *const u8,
    value_len: usize,
) {
    let configmaton = &mut *(configmaton as *mut MyConfigmaton);
    let key = std::slice::from_raw_parts(key, key_len);
    let value = std::slice::from_raw_parts(value, value_len);
    configmaton.set(key, value);
}

#[no_mangle]
pub unsafe extern "C" fn configmaton_get(
    configmaton: *const FfiConfigmaton,
    key: *const u8,
    key_len: usize,
) -> Bytestring {
    let configmaton = &*(configmaton as *mut MyConfigmaton);
    let key = std::slice::from_raw_parts(key, key_len);
    let result = configmaton.get(key);
    match result {
        Some(value) => Bytestring { data: value.as_ptr(), len: value.len() },
        None => Bytestring { data: std::ptr::null(), len: std::usize::MAX },
    }
}

#[repr(C)]
pub struct Bytestring {
    pub len: usize,
    pub data: *const u8,
}

#[no_mangle]
pub unsafe extern "C" fn configmaton_pop_command(configmaton: *mut FfiConfigmaton) -> Bytestring {
    let configmaton = &mut *(configmaton as *mut MyConfigmaton);
    match configmaton.pop_command() {
        Some(command) => Bytestring { data: command.as_ptr(), len: command.len() },
        None => Bytestring { data: std::ptr::null(), len: std::usize::MAX },
    }
}

#[no_mangle]
pub unsafe extern "C" fn configmaton_clear_children(configmaton: *mut FfiConfigmaton) {
    let configmaton = &mut *(configmaton as *mut MyConfigmaton);
    configmaton.clear_children();
}
