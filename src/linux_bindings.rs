#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBindingType {
    Str,
    StrCallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBindingReturn {
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxBindingParam {
    pub name: &'static str,
    pub signature: &'static str,
    pub ty: LinuxBindingType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxBinding {
    pub name: &'static str,
    pub params: &'static [LinuxBindingParam],
    pub returns: LinuxBindingReturn,
}

const SECURE_KEY_VALUE: &[LinuxBindingParam] = &[
    LinuxBindingParam {
        name: "key",
        signature: "key: str",
        ty: LinuxBindingType::Str,
    },
    LinuxBindingParam {
        name: "value",
        signature: "value: str",
        ty: LinuxBindingType::Str,
    },
];

const SECURE_KEY_CALLBACK: &[LinuxBindingParam] = &[
    LinuxBindingParam {
        name: "key",
        signature: "key: str",
        ty: LinuxBindingType::Str,
    },
    LinuxBindingParam {
        name: "callback",
        signature: "callback: fn(str) -> void",
        ty: LinuxBindingType::StrCallback,
    },
];

const SECURE_KEY: &[LinuxBindingParam] = &[LinuxBindingParam {
    name: "key",
    signature: "key: str",
    ty: LinuxBindingType::Str,
}];

pub const LINUX_BINDINGS: &[LinuxBinding] = &[
    LinuxBinding {
        name: "secureStore",
        params: SECURE_KEY_VALUE,
        returns: LinuxBindingReturn::Bool,
    },
    LinuxBinding {
        name: "secureRead",
        params: SECURE_KEY_CALLBACK,
        returns: LinuxBindingReturn::Bool,
    },
    LinuxBinding {
        name: "secureRemove",
        params: SECURE_KEY,
        returns: LinuxBindingReturn::Bool,
    },
];

pub fn binding_named(name: &str) -> Option<&'static LinuxBinding> {
    LINUX_BINDINGS.iter().find(|binding| binding.name == name)
}

impl LinuxBinding {
    pub fn return_name(&self) -> &'static str {
        match self.returns {
            LinuxBindingReturn::Bool => "bool",
        }
    }

    pub fn runtime_symbol(&self) -> String {
        let mut name = String::from("flux__linux_");
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
    fn linux_binding_names_and_runtime_symbols_are_unique() {
        let mut names = HashSet::new();
        let mut symbols = HashSet::new();
        for binding in LINUX_BINDINGS {
            assert!(names.insert(binding.name));
            assert!(symbols.insert(binding.runtime_symbol()));
        }
    }
}
