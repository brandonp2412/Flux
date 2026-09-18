#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsBindingType {
    I64,
    Str,
    StrCallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsBindingReturn {
    I64,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsBindingParam {
    pub name: &'static str,
    pub signature: &'static str,
    pub ty: WindowsBindingType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsBinding {
    pub name: &'static str,
    pub params: &'static [WindowsBindingParam],
    pub returns: WindowsBindingReturn,
}

const NO_PARAMS: &[WindowsBindingParam] = &[];
const TITLE_MESSAGE: &[WindowsBindingParam] = &[
    WindowsBindingParam {
        name: "title",
        signature: "title: str",
        ty: WindowsBindingType::Str,
    },
    WindowsBindingParam {
        name: "message",
        signature: "message: str",
        ty: WindowsBindingType::Str,
    },
];
const TARGET: &[WindowsBindingParam] = &[WindowsBindingParam {
    name: "target",
    signature: "target: str",
    ty: WindowsBindingType::Str,
}];
const BEEP: &[WindowsBindingParam] = &[
    WindowsBindingParam {
        name: "frequencyHz",
        signature: "frequencyHz: i64",
        ty: WindowsBindingType::I64,
    },
    WindowsBindingParam {
        name: "durationMs",
        signature: "durationMs: i64",
        ty: WindowsBindingType::I64,
    },
];
const SECURE_KEY_VALUE: &[WindowsBindingParam] = &[
    WindowsBindingParam {
        name: "key",
        signature: "key: str",
        ty: WindowsBindingType::Str,
    },
    WindowsBindingParam {
        name: "value",
        signature: "value: str",
        ty: WindowsBindingType::Str,
    },
];
const SECURE_KEY_CALLBACK: &[WindowsBindingParam] = &[
    WindowsBindingParam {
        name: "key",
        signature: "key: str",
        ty: WindowsBindingType::Str,
    },
    WindowsBindingParam {
        name: "callback",
        signature: "callback: fn(str) -> void",
        ty: WindowsBindingType::StrCallback,
    },
];
const SECURE_KEY: &[WindowsBindingParam] = &[WindowsBindingParam {
    name: "key",
    signature: "key: str",
    ty: WindowsBindingType::Str,
}];

pub const WINDOWS_BINDINGS: &[WindowsBinding] = &[
    WindowsBinding {
        name: "messageBox",
        params: TITLE_MESSAGE,
        returns: WindowsBindingReturn::I64,
    },
    WindowsBinding {
        name: "open",
        params: TARGET,
        returns: WindowsBindingReturn::Bool,
    },
    WindowsBinding {
        name: "beep",
        params: BEEP,
        returns: WindowsBindingReturn::Bool,
    },
    WindowsBinding {
        name: "screenWidth",
        params: NO_PARAMS,
        returns: WindowsBindingReturn::I64,
    },
    WindowsBinding {
        name: "screenHeight",
        params: NO_PARAMS,
        returns: WindowsBindingReturn::I64,
    },
    WindowsBinding {
        name: "secureStore",
        params: SECURE_KEY_VALUE,
        returns: WindowsBindingReturn::Bool,
    },
    WindowsBinding {
        name: "secureRead",
        params: SECURE_KEY_CALLBACK,
        returns: WindowsBindingReturn::Bool,
    },
    WindowsBinding {
        name: "secureRemove",
        params: SECURE_KEY,
        returns: WindowsBindingReturn::Bool,
    },
];

pub fn binding_named(name: &str) -> Option<&'static WindowsBinding> {
    WINDOWS_BINDINGS.iter().find(|binding| binding.name == name)
}

impl WindowsBinding {
    pub fn return_name(&self) -> &'static str {
        match self.returns {
            WindowsBindingReturn::I64 => "i64",
            WindowsBindingReturn::Bool => "bool",
        }
    }

    pub fn runtime_symbol(&self) -> String {
        let mut name = String::from("flux__windows_");
        for character in self.name.chars() {
            if character.is_ascii_uppercase() {
                name.push('_');
                name.push(character.to_ascii_lowercase());
            } else {
                name.push(character);
            }
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn windows_binding_names_and_runtime_symbols_are_unique() {
        let mut names = HashSet::new();
        let mut symbols = HashSet::new();
        for binding in WINDOWS_BINDINGS {
            assert!(names.insert(binding.name));
            assert!(symbols.insert(binding.runtime_symbol()));
        }
        assert_eq!(
            binding_named("messageBox").unwrap().runtime_symbol(),
            "flux__windows_message_box"
        );
    }
}
