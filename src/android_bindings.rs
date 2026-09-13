#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidBindingType {
    I64,
    Bool,
    Str,
    StrCallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidBindingReturn {
    Void,
    I64,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidBindingParam {
    pub name: &'static str,
    pub signature: &'static str,
    pub ty: AndroidBindingType,
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidBinding {
    pub name: &'static str,
    pub params: &'static [AndroidBindingParam],
    pub returns: AndroidBindingReturn,
    pub minimum_sdk: u32,
    pub optional_runtime_suffix: Option<&'static str>,
}

const I64_JOB_ID: AndroidBindingParam = AndroidBindingParam {
    name: "jobId",
    signature: "jobId: i64",
    ty: AndroidBindingType::I64,
    optional: false,
};
const STR_CALLBACK: AndroidBindingParam = AndroidBindingParam {
    name: "callback",
    signature: "callback: fn(str) -> void",
    ty: AndroidBindingType::StrCallback,
    optional: false,
};

const NO_PARAMS: &[AndroidBindingParam] = &[];
const FEATURE: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "feature",
    signature: "feature: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const DURATION: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "durationMs",
    signature: "durationMs: i64",
    ty: AndroidBindingType::I64,
    optional: false,
}];
const ENABLED: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "enabled",
    signature: "enabled: bool",
    ty: AndroidBindingType::Bool,
    optional: false,
}];
const JOB: &[AndroidBindingParam] = &[
    I64_JOB_ID,
    AndroidBindingParam {
        name: "delayMs",
        signature: "delayMs: i64",
        ty: AndroidBindingType::I64,
        optional: false,
    },
];
const JOB_ID: &[AndroidBindingParam] = &[I64_JOB_ID];
const URL: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "url",
    signature: "url: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const TEXT: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "text",
    signature: "text: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const PATH: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "path",
    signature: "path: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const SECURE_KEY_VALUE: &[AndroidBindingParam] = &[
    AndroidBindingParam {
        name: "key",
        signature: "key: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "value",
        signature: "value: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
];
const SECURE_KEY_CALLBACK: &[AndroidBindingParam] = &[
    AndroidBindingParam {
        name: "key",
        signature: "key: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    STR_CALLBACK,
];
const SECURE_KEY: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "key",
    signature: "key: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const WRAP: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "wrap",
    signature: "wrap: bool = false",
    ty: AndroidBindingType::Bool,
    optional: true,
}];
const POSITION: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "position",
    signature: "position: i64",
    ty: AndroidBindingType::I64,
    optional: false,
}];
const SELECTION: &[AndroidBindingParam] = &[
    AndroidBindingParam {
        name: "start",
        signature: "start: i64",
        ty: AndroidBindingType::I64,
        optional: false,
    },
    AndroidBindingParam {
        name: "end",
        signature: "end: i64",
        ty: AndroidBindingType::I64,
        optional: false,
    },
];
const ACTION: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "action",
    signature: "action: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const CALLBACK: &[AndroidBindingParam] = &[STR_CALLBACK];
const CHANNEL: &[AndroidBindingParam] = &[
    AndroidBindingParam {
        name: "id",
        signature: "id: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "name",
        signature: "name: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "description",
        signature: "description: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
];
const PERMISSION: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "permission",
    signature: "permission: str",
    ty: AndroidBindingType::Str,
    optional: false,
}];
const NOTIFY: &[AndroidBindingParam] = &[
    AndroidBindingParam {
        name: "channelId",
        signature: "channelId: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "notificationId",
        signature: "notificationId: i64",
        ty: AndroidBindingType::I64,
        optional: false,
    },
    AndroidBindingParam {
        name: "title",
        signature: "title: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "body",
        signature: "body: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
];
const NOTIFY_URL_ACTION: &[AndroidBindingParam] = &[
    NOTIFY[0],
    NOTIFY[1],
    NOTIFY[2],
    NOTIFY[3],
    AndroidBindingParam {
        name: "actionLabel",
        signature: "actionLabel: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
    AndroidBindingParam {
        name: "url",
        signature: "url: str",
        ty: AndroidBindingType::Str,
        optional: false,
    },
];
const NOTIFICATION_ID: &[AndroidBindingParam] = &[AndroidBindingParam {
    name: "notificationId",
    signature: "notificationId: i64",
    ty: AndroidBindingType::I64,
    optional: false,
}];

macro_rules! binding {
    ($name:literal, $params:expr, $returns:ident) => {
        AndroidBinding {
            name: $name,
            params: $params,
            returns: AndroidBindingReturn::$returns,
            minimum_sdk: 21,
            optional_runtime_suffix: None,
        }
    };
}

macro_rules! binding_min_sdk {
    ($name:literal, $params:expr, $returns:ident, $minimum_sdk:literal) => {
        AndroidBinding {
            name: $name,
            params: $params,
            returns: AndroidBindingReturn::$returns,
            minimum_sdk: $minimum_sdk,
            optional_runtime_suffix: None,
        }
    };
}

macro_rules! binding_with_optional_runtime_suffix {
    ($name:literal, $params:expr, $returns:ident, $suffix:literal) => {
        AndroidBinding {
            name: $name,
            params: $params,
            returns: AndroidBindingReturn::$returns,
            minimum_sdk: 21,
            optional_runtime_suffix: Some($suffix),
        }
    };
}

pub const ANDROID_BINDINGS: &[AndroidBinding] = &[
    binding!("sdkInt", NO_PARAMS, I64),
    binding!("hasSystemFeature", FEATURE, Bool),
    binding!("vibrate", DURATION, Void),
    binding!("keepScreenOn", ENABLED, Void),
    binding!("finishActivity", NO_PARAMS, Void),
    binding!("scheduleBackgroundJob", JOB, Bool),
    binding!("cancelBackgroundJob", JOB_ID, Void),
    binding!("openUrl", URL, Void),
    binding!("openAppSettings", NO_PARAMS, Void),
    binding!("openNotificationSettings", NO_PARAMS, Void),
    binding!("share", TEXT, Void),
    binding!("setClipboardText", TEXT, Void),
    binding!("startMicrophoneRecording", PATH, Bool),
    binding!("stopMicrophoneRecording", NO_PARAMS, Bool),
    binding_min_sdk!("secureStore", SECURE_KEY_VALUE, Bool, 23),
    binding_min_sdk!("secureRead", SECURE_KEY_CALLBACK, Bool, 23),
    binding_min_sdk!("secureRemove", SECURE_KEY, Bool, 23),
    binding!("showKeyboard", NO_PARAMS, Void),
    binding!("hideKeyboard", NO_PARAMS, Void),
    binding_with_optional_runtime_suffix!("focusNext", WRAP, Void, "_wrap"),
    binding_with_optional_runtime_suffix!("focusPrevious", WRAP, Void, "_wrap"),
    binding!("focusFirst", NO_PARAMS, Void),
    binding!("focusLast", NO_PARAMS, Void),
    binding!("clearFocus", NO_PARAMS, Void),
    binding!("selectionStart", NO_PARAMS, I64),
    binding!("selectionEnd", NO_PARAMS, I64),
    binding!("setCaret", POSITION, Bool),
    binding!("setSelection", SELECTION, Bool),
    binding!("setImeAction", ACTION, Bool),
    binding!("pickFile", CALLBACK, Void),
    binding!("pickMedia", CALLBACK, Void),
    binding!("pickDirectory", CALLBACK, Void),
    binding!("createNotificationChannel", CHANNEL, Void),
    binding!("permissionGranted", PERMISSION, Bool),
    binding!("requestPermission", PERMISSION, Void),
    binding!("notificationPermissionGranted", NO_PARAMS, Bool),
    binding!("requestNotificationPermission", NO_PARAMS, Void),
    binding!("notify", NOTIFY, Void),
    binding!("notifyUrlAction", NOTIFY_URL_ACTION, Void),
    binding!("cancelNotification", NOTIFICATION_ID, Void),
];

pub fn binding_named(name: &str) -> Option<&'static AndroidBinding> {
    ANDROID_BINDINGS.iter().find(|binding| binding.name == name)
}

pub fn first_unavailable_reachable_binding(
    generated_c: &str,
    min_sdk: u32,
) -> Option<&'static AndroidBinding> {
    ANDROID_BINDINGS.iter().find(|binding| {
        binding.minimum_sdk > min_sdk && generated_c.contains(&binding.runtime_symbol_stem())
    })
}

impl AndroidBinding {
    pub fn required_param_count(&self) -> usize {
        self.params.iter().filter(|param| !param.optional).count()
    }

    pub fn return_name(&self) -> &'static str {
        match self.returns {
            AndroidBindingReturn::Void => "void",
            AndroidBindingReturn::I64 => "i64",
            AndroidBindingReturn::Bool => "bool",
        }
    }

    pub fn signature(&self) -> String {
        let params = self
            .params
            .iter()
            .map(|param| param.signature)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "fn android.{}({params}) -> {}",
            self.name,
            self.return_name()
        )
    }

    pub fn runtime_symbol_stem(&self) -> String {
        let mut name = String::from("flux__android_");
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

    pub fn runtime_symbol_for_arity(&self, arity: usize) -> Option<String> {
        let required = self.required_param_count();
        if arity < required || arity > self.params.len() {
            return None;
        }
        let mut symbol = self.runtime_symbol_stem();
        if arity > required {
            symbol.push_str(self.optional_runtime_suffix?);
        }
        Some(symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn android_binding_names_are_unique_and_supported_by_the_android_floor() {
        let mut names = HashSet::new();
        for binding in ANDROID_BINDINGS {
            assert!(names.insert(binding.name));
            assert!(binding.minimum_sdk >= 21);
            let required = binding.required_param_count();
            assert!(required <= binding.params.len());
            assert!(binding.runtime_symbol_for_arity(required).is_some());
            assert!(
                (required..=binding.params.len())
                    .all(|arity| binding.runtime_symbol_for_arity(arity).is_some())
            );
            if required > 0 {
                assert!(binding.runtime_symbol_for_arity(required - 1).is_none());
            }
            assert!(
                binding
                    .runtime_symbol_for_arity(binding.params.len() + 1)
                    .is_none()
            );
        }
    }

    #[test]
    fn android_binding_signatures_preserve_optional_parameters() {
        assert_eq!(
            binding_named("focusNext").unwrap().signature(),
            "fn android.focusNext(wrap: bool = false) -> void"
        );
        assert_eq!(
            binding_named("pickFile").unwrap().signature(),
            "fn android.pickFile(callback: fn(str) -> void) -> void"
        );
        assert_eq!(
            binding_named("startMicrophoneRecording")
                .unwrap()
                .signature(),
            "fn android.startMicrophoneRecording(path: str) -> bool"
        );
        assert_eq!(
            binding_named("secureRead").unwrap().signature(),
            "fn android.secureRead(key: str, callback: fn(str) -> void) -> bool"
        );
        assert_eq!(binding_named("secureStore").unwrap().minimum_sdk, 23);
        assert_eq!(
            binding_named("notifyUrlAction")
                .unwrap()
                .runtime_symbol_stem(),
            "flux__android_notify_url_action"
        );
        assert_eq!(
            binding_named("focusNext")
                .unwrap()
                .runtime_symbol_for_arity(0)
                .as_deref(),
            Some("flux__android_focus_next")
        );
        assert_eq!(
            binding_named("focusNext")
                .unwrap()
                .runtime_symbol_for_arity(1)
                .as_deref(),
            Some("flux__android_focus_next_wrap")
        );
        assert_eq!(
            binding_named("notify")
                .unwrap()
                .runtime_symbol_for_arity(4)
                .as_deref(),
            Some("flux__android_notify")
        );
        assert!(
            binding_named("notify")
                .unwrap()
                .runtime_symbol_for_arity(3)
                .is_none()
        );
    }

    #[test]
    fn android_binding_availability_checks_only_reachable_platform_calls() {
        assert_eq!(
            first_unavailable_reachable_binding("flux__android_vibrate(25);", 20)
                .map(|binding| binding.name),
            Some("vibrate")
        );
        assert!(first_unavailable_reachable_binding("flux__fn_unused();", 20).is_none());
        assert!(first_unavailable_reachable_binding("flux__android_vibrate(25);", 21).is_none());
    }
}
