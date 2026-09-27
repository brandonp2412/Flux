/// Compatibility version for the Flux platform-support tier contract.
pub const PLATFORM_SUPPORT_POLICY_VERSION: u32 = 1;

/// Schema version emitted by the platforms JSON command.
pub const PLATFORM_SUPPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformSupportTier {
    Validated,
    Preview,
    Unavailable,
}

impl PlatformSupportTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validated => "validated",
            Self::Preview => "preview",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformSupport {
    pub target: &'static str,
    pub tier: PlatformSupportTier,
    pub scope: &'static str,
}

pub const PLATFORM_SUPPORT: &[PlatformSupport] = &[
    PlatformSupport {
        target: "linux",
        tier: PlatformSupportTier::Validated,
        scope: "native-app",
    },
    PlatformSupport {
        target: "android",
        tier: PlatformSupportTier::Validated,
        scope: "native-app",
    },
    PlatformSupport {
        target: "windows",
        tier: PlatformSupportTier::Preview,
        scope: "native-app",
    },
    PlatformSupport {
        target: "web",
        tier: PlatformSupportTier::Preview,
        scope: "browser",
    },
    PlatformSupport {
        target: "headless/server",
        tier: PlatformSupportTier::Validated,
        scope: "linux-native",
    },
    PlatformSupport {
        target: "ios",
        tier: PlatformSupportTier::Unavailable,
        scope: "none",
    },
    PlatformSupport {
        target: "macos",
        tier: PlatformSupportTier::Unavailable,
        scope: "none",
    },
];

pub fn platform_support_json() -> String {
    let mut output = format!(
        "{{\"schema_version\":{},\"policy_version\":{},\"targets\":[",
        PLATFORM_SUPPORT_SCHEMA_VERSION, PLATFORM_SUPPORT_POLICY_VERSION
    );
    for (index, support) in PLATFORM_SUPPORT.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&format!(
            "{{\"target\":\"{}\",\"tier\":\"{}\",\"scope\":\"{}\"}}",
            support.target,
            support.tier.as_str(),
            support.scope
        ));
    }
    output.push_str("]}");
    output
}
