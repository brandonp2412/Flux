pub fn qualified<'a>(namespace: &str, name: &'a str) -> &'a str {
    match (namespace, name) {
        ("process", "parentPid") => "parent",
        ("process", "cpuMillis") => "cpu",
        ("process", "peakResidentMemoryBytes") => "memory",
        ("process", "terminationRequested") => "stopping",
        ("sqlite", "execute") => "run",
        ("net", "acceptTimeout") | ("net", "acceptMany") | ("net", "acceptManyTimeout") => "accept",
        ("net", "writeTimeout")
        | ("net", "writeFrom")
        | ("net", "writeFromTimeout")
        | ("net", "writeParts")
        | ("net", "writePartsFrom")
        | ("net", "writePartsTimeout")
        | ("net", "writePartsFromTimeout")
        | ("net", "writeTo")
        | ("net", "writePartsTo") => "write",
        ("net", "readTimeout")
        | ("net", "readMany")
        | ("net", "readManyTimeout")
        | ("net", "readFrom")
        | ("net", "readFromTimeout")
        | ("net", "readManyFrom")
        | ("net", "readManyFromTimeout") => "read",
        ("net", "nonblocking") => "blocking",
        ("net", "keepAlive") => "alive",
        ("net", "readableMany") => "readable",
        ("net", "writableMany") => "writable",
        ("net", "readyMany") => "ready",
        ("net", "closeRead") | ("net", "closeWrite") => "close",
        ("net", "tcpConnect") => "connect",
        ("net", "tcpListen") => "listen",
        ("net", "tcpAccept") => "accept",
        ("net", "tcpAcceptWithTimeout") => "accept",
        ("net", "tcpAcceptMany") => "accept",
        ("net", "tcpAcceptManyWithTimeout") => "accept",
        ("net", "udpConnect") => "udp",
        ("net", "udpBind") => "bind",
        ("net", "localPort") => "port",
        ("net", "peerAddress") => "peer",
        ("net", "localAddress") => "local",
        ("net", "sendText") => "write",
        ("net", "sendTextWithTimeout") => "write",
        ("net", "sendTextProgress") => "write",
        ("net", "sendTextProgressWithTimeout") => "write",
        ("net", "sendTextParts") => "write",
        ("net", "sendTextPartsProgress") => "write",
        ("net", "sendTextPartsWithTimeout") => "write",
        ("net", "sendTextPartsProgressWithTimeout") => "write",
        ("net", "sendTextTo") => "write",
        ("net", "sendTextToParts") => "write",
        ("net", "receiveText") => "read",
        ("net", "receiveTextWithTimeout") => "read",
        ("net", "receiveTextMany") => "read",
        ("net", "receiveTextManyWithTimeout") => "read",
        ("net", "receiveTextFrom") => "read",
        ("net", "receiveTextFromWithTimeout") => "read",
        ("net", "receiveTextFromMany") => "read",
        ("net", "receiveTextFromManyWithTimeout") => "read",
        ("net", "setNonblocking") => "blocking",
        ("net", "setNoDelay") => "noDelay",
        ("net", "setKeepAlive") => "alive",
        ("net", "waitReadable") => "readable",
        ("net", "waitWritable") => "writable",
        ("net", "waitReadableMany") => "readable",
        ("net", "waitWritableMany") => "writable",
        ("net", "waitReadyMany") => "ready",
        ("net", "shutdownRead") => "close",
        ("net", "shutdownWrite") => "close",
        ("http", "readRequest") | ("http", "readRequestHeaders") | ("http", "readRequestBody") => {
            "read"
        }
        ("http", "readResponse") | ("http", "readResponseBody") => "response",
        ("http", "requestHeaders") => "request",
        ("http", "respondHeaders") => "respond",
        ("http", "receiveRequestHead") => "read",
        ("http", "receiveRequestHeadWithHeaders") => "read",
        ("http", "receiveRequestWithTextBody") => "read",
        ("http", "receiveResponseHeadWithHeaders") => "response",
        ("http", "receiveResponseWithTextBody") => "response",
        ("http", "sendTextRequest") => "request",
        ("http", "requestWithHeaders") | ("http", "sendTextRequestWithHeaders") => "request",
        ("http", "sendTextResponse") => "respond",
        ("http", "respondWithHeaders") | ("http", "sendTextResponseWithHeaders") => "respond",
        ("http", "serveOnce") => "once",
        ("http", "serveConcurrent") | ("http", "serveConcurrentLimit") => "serve",
        ("url", "decodeForm") => "decode",
        ("url", "encodeForm") => "encode",
        ("url", "parseHttp") => "parse",
        ("url", "parseFormQuery") => "query",
        ("url", "decodeComponent") => "decode",
        ("url", "encodeComponent") => "encode",
        ("url", "decodeFormComponent") => "decode",
        ("url", "encodeFormComponent") => "encode",
        ("locale", "formatNumber") => "number",
        ("locale", "formatDateTime") => "date",
        ("locale", "formatCurrency") => "currency",
        ("worker", "joinAll") => "join",
        ("worker", "cancelAll") => "cancel",
        ("worker", "startWith") => "start",
        ("worker", "joinChildren") => "join",
        ("worker", "cancelChildren") => "cancel",
        ("worker", "cancelled") => "stopped",
        ("channel", "create") => "open",
        ("channel", "send") => "write",
        ("channel", "receive") => "read",
        ("time", "monotonic") => "steady",
        ("time", "sleepUntil") => "until",
        ("time", "dayOfYear") => "yearday",
        ("time", "unixMillis") => "now",
        ("time", "monotonicMillis") => "steady",
        ("time", "sleepMillis") => "sleep",
        ("time", "sleepUntilMonotonic") => "until",
        ("time", "utcUnixMillis") => "utc",
        ("time", "utcYear") => "year",
        ("time", "utcMonth") => "month",
        ("time", "utcDay") => "day",
        ("time", "utcHour") => "hour",
        ("time", "utcMinute") => "minute",
        ("time", "utcSecond") => "second",
        ("time", "utcMillisecond") => "millis",
        ("time", "utcWeekday") => "weekday",
        ("time", "utcDayOfYear") => "yearday",
        ("file", "modifiedUnixMillis") => "modified",
        ("file", "setModified") => "modified",
        ("file", "accessed") | ("file", "setAccessed") => "accessed",
        ("file", "permissions") | ("file", "setPermissions") => "mode",
        ("file", "allocatedSize") => "space",
        ("file", "hardLinks") => "links",
        ("file", "blockSize") => "block",
        ("directory", "modifiedUnixMillis") => "modified",
        ("directory", "setModified") => "modified",
        ("directory", "accessed") | ("directory", "setAccessed") => "accessed",
        ("directory", "permissions") | ("directory", "setPermissions") => "mode",
        ("directory", "allocatedSize") => "space",
        ("directory", "hardLinks") => "links",
        ("directory", "blockSize") => "block",
        ("directory", "createAll") => "make",
        ("directory", "removeAll") => "erase",
        ("frame", "request") => "next",
        ("clipboard", "setText") => "write",
        ("clipboard", "readText") => "read",
        ("fileDialog", "openFile") => "open",
        ("fileDialog", "saveFile") => "save",
        ("fileDialog", "selectDirectory") => "folder",
        ("focus", "previous") => "prior",
        ("focus", "previousIn") => "priorIn",
        ("textInput", "selectionStart") => "start",
        ("textInput", "selectionEnd") => "end",
        ("textInput", "setCaret") => "caret",
        ("textInput", "setSelection") => "select",
        ("android", "noticeSettings") => "notices",
        ("android", "clipboard") => "copy",
        ("android", "noticeAllowed") => "canNotify",
        ("android", "askNotice") => "askNotify",
        ("android", "notifyUrl") => "notify",
        ("android", "cancelNotice") => "unnotify",
        ("android", "stopRecord") => "record",
        ("android", "sdkInt") => "sdk",
        ("android", "hasSystemFeature") => "feature",
        ("android", "keepScreenOn") => "awake",
        ("android", "finishActivity") => "finish",
        ("android", "scheduleBackgroundJob") => "schedule",
        ("android", "cancelBackgroundJob") => "cancelJob",
        ("android", "enqueueWork") => "work",
        ("android", "openUrl") => "open",
        ("android", "openAppSettings") => "settings",
        ("android", "openNotificationSettings") => "notices",
        ("android", "setClipboardText") => "copy",
        ("android", "focusNext") => "next",
        ("android", "focusPrevious") => "prior",
        ("android", "focusFirst") => "first",
        ("android", "focusLast") => "last",
        ("android", "clearFocus") => "blur",
        ("android", "selectionStart") => "start",
        ("android", "selectionEnd") => "end",
        ("android", "setCaret") => "caret",
        ("android", "setSelection") => "select",
        ("android", "setImeAction") => "ime",
        ("android", "pickFile") => "file",
        ("android", "pickMedia") => "media",
        ("android", "pickDirectory") => "folder",
        ("android", "createNotificationChannel") => "channel",
        ("android", "permissionGranted") => "allowed",
        ("android", "requestPermission") => "ask",
        ("android", "notificationPermissionGranted") => "canNotify",
        ("android", "requestNotificationPermission") => "askNotify",
        ("android", "notifyUrlAction") => "notify",
        ("android", "cancelNotification") => "unnotify",
        ("android", "startMicrophoneRecording") => "record",
        ("android", "stopMicrophoneRecording") => "record",
        ("android", "showKeyboard") | ("android", "hideKeyboard") => "keyboard",
        ("android", "secureStore") => "store",
        ("android", "secureRead") => "load",
        ("android", "secureRemove") => "erase",
        _ => name,
    }
}

pub fn qualified_impl<'a>(namespace: &str, name: &'a str) -> &'a str {
    match (namespace, name) {
        ("process", "parent") => "parentPid",
        ("process", "cpu") => "cpuMillis",
        ("process", "memory") => "peakResidentMemoryBytes",
        ("process", "stopping") => "terminationRequested",
        ("sqlite", "run") => "execute",
        ("net", "udp") => "udpConnect",
        ("net", "bind") => "udpBind",
        ("net", "acceptTimeout") => "tcpAcceptWithTimeout",
        ("net", "acceptManyTimeout") => "tcpAcceptManyWithTimeout",
        ("net", "port") => "localPort",
        ("net", "peer") => "peerAddress",
        ("net", "local") => "localAddress",
        ("net", "write") => "sendText",
        ("net", "writeTimeout") => "sendTextWithTimeout",
        ("net", "writeFrom") => "sendTextProgress",
        ("net", "writeFromTimeout") => "sendTextProgressWithTimeout",
        ("net", "writeParts") => "sendTextParts",
        ("net", "writePartsFrom") => "sendTextPartsProgress",
        ("net", "writePartsTimeout") => "sendTextPartsWithTimeout",
        ("net", "writePartsFromTimeout") => "sendTextPartsProgressWithTimeout",
        ("net", "writeTo") => "sendTextTo",
        ("net", "writePartsTo") => "sendTextToParts",
        ("net", "read") => "receiveText",
        ("net", "readTimeout") => "receiveTextWithTimeout",
        ("net", "readMany") => "receiveTextMany",
        ("net", "readManyTimeout") => "receiveTextManyWithTimeout",
        ("net", "readFrom") => "receiveTextFrom",
        ("net", "readFromTimeout") => "receiveTextFromWithTimeout",
        ("net", "readManyFrom") => "receiveTextFromMany",
        ("net", "readManyFromTimeout") => "receiveTextFromManyWithTimeout",
        ("net", "nonblocking") => "setNonblocking",
        ("net", "blocking") => "setNonblocking",
        ("net", "noDelay") => "setNoDelay",
        ("net", "keepAlive") => "setKeepAlive",
        ("net", "alive") => "setKeepAlive",
        ("net", "readable") => "waitReadable",
        ("net", "writable") => "waitWritable",
        ("net", "readableMany") => "waitReadableMany",
        ("net", "writableMany") => "waitWritableMany",
        ("net", "readyMany") => "waitReadyMany",
        ("net", "ready") => "waitReadyMany",
        ("net", "closeRead") => "shutdownRead",
        ("net", "closeWrite") => "shutdownWrite",
        ("http", "read") => "receiveRequestHead",
        ("http", "readRequest") => "receiveRequestHead",
        ("http", "readRequestHeaders") => "receiveRequestHeadWithHeaders",
        ("http", "readRequestBody") => "receiveRequestWithTextBody",
        ("http", "readResponse") => "receiveResponseHeadWithHeaders",
        ("http", "readResponseBody") => "receiveResponseWithTextBody",
        ("http", "requestHeaders") => "requestWithHeaders",
        ("http", "respondHeaders") => "respondWithHeaders",
        ("http", "once") => "serveOnce",
        ("http", "response") => "receiveResponseHeadWithHeaders",
        ("url", "parse") => "parseHttp",
        ("url", "query") => "parseFormQuery",
        ("url", "decode") => "decodeComponent",
        ("url", "encode") => "encodeComponent",
        ("url", "decodeForm") => "decodeFormComponent",
        ("url", "encodeForm") => "encodeFormComponent",
        ("locale", "number") => "formatNumber",
        ("locale", "date") => "formatDateTime",
        ("locale", "currency") => "formatCurrency",
        ("worker", "joinAll") => "joinChildren",
        ("worker", "cancelAll") => "cancelChildren",
        ("worker", "stopped") => "cancelled",
        ("channel", "open") => "create",
        ("channel", "write") => "send",
        ("channel", "read") => "receive",
        ("time", "now") => "unixMillis",
        ("time", "monotonic") => "monotonicMillis",
        ("time", "steady") => "monotonicMillis",
        ("time", "sleepUntil") => "sleepUntilMonotonic",
        ("time", "until") => "sleepUntilMonotonic",
        ("time", "utc") => "utcUnixMillis",
        ("time", "year") => "utcYear",
        ("time", "month") => "utcMonth",
        ("time", "day") => "utcDay",
        ("time", "hour") => "utcHour",
        ("time", "minute") => "utcMinute",
        ("time", "second") => "utcSecond",
        ("time", "millis") => "utcMillisecond",
        ("time", "weekday") => "utcWeekday",
        ("time", "dayOfYear") => "utcDayOfYear",
        ("time", "yearday") => "utcDayOfYear",
        ("file", "modified") => "modifiedUnixMillis",
        ("file", "mode") => "permissions",
        ("file", "space") => "allocatedSize",
        ("file", "links") => "hardLinks",
        ("file", "block") => "blockSize",
        ("directory", "modified") => "modifiedUnixMillis",
        ("directory", "mode") => "permissions",
        ("directory", "space") => "allocatedSize",
        ("directory", "links") => "hardLinks",
        ("directory", "block") => "blockSize",
        ("directory", "make") => "createAll",
        ("directory", "erase") => "removeAll",
        ("frame", "next") => "request",
        ("fileDialog", "folder") => "selectDirectory",
        ("focus", "prior") => "previous",
        ("focus", "priorIn") => "previousIn",
        ("textInput", "start") => "selectionStart",
        ("textInput", "end") => "selectionEnd",
        ("textInput", "caret") => "setCaret",
        ("textInput", "select") => "setSelection",
        ("android", "sdk") => "sdkInt",
        ("android", "feature") => "hasSystemFeature",
        ("android", "awake") => "keepScreenOn",
        ("android", "finish") => "finishActivity",
        ("android", "schedule") => "scheduleBackgroundJob",
        ("android", "cancelJob") => "cancelBackgroundJob",
        ("android", "work") => "enqueueWork",
        ("android", "open") => "openUrl",
        ("android", "settings") => "openAppSettings",
        ("android", "noticeSettings") => "openNotificationSettings",
        ("android", "notices") => "openNotificationSettings",
        ("android", "clipboard") => "setClipboardText",
        ("android", "copy") => "setClipboardText",
        ("android", "next") => "focusNext",
        ("android", "prior") => "focusPrevious",
        ("android", "first") => "focusFirst",
        ("android", "last") => "focusLast",
        ("android", "blur") => "clearFocus",
        ("android", "start") => "selectionStart",
        ("android", "end") => "selectionEnd",
        ("android", "caret") => "setCaret",
        ("android", "select") => "setSelection",
        ("android", "ime") => "setImeAction",
        ("android", "file") => "pickFile",
        ("android", "media") => "pickMedia",
        ("android", "folder") => "pickDirectory",
        ("android", "channel") => "createNotificationChannel",
        ("android", "allowed") => "permissionGranted",
        ("android", "ask") => "requestPermission",
        ("android", "noticeAllowed") => "notificationPermissionGranted",
        ("android", "canNotify") => "notificationPermissionGranted",
        ("android", "askNotice") => "requestNotificationPermission",
        ("android", "askNotify") => "requestNotificationPermission",
        ("android", "notifyUrl") => "notifyUrlAction",
        ("android", "cancelNotice") => "cancelNotification",
        ("android", "unnotify") => "cancelNotification",
        ("android", "record") => "startMicrophoneRecording",
        ("android", "stopRecord") => "stopMicrophoneRecording",
        ("android", "store") => "secureStore",
        ("android", "load") => "secureRead",
        ("android", "erase") => "secureRemove",
        _ => name,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedArgSource {
    Positional(usize),
    Named(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedCallPlan {
    pub implementation: &'static str,
    pub args: Vec<QualifiedArgSource>,
    pub markers: Vec<&'static str>,
}

fn has_named(named: &[&str], expected: &[&str]) -> bool {
    named.len() == expected.len() && expected.iter().all(|name| named.contains(name))
}

fn plan(
    implementation: &'static str,
    args: Vec<QualifiedArgSource>,
    markers: Vec<&'static str>,
) -> Option<QualifiedCallPlan> {
    Some(QualifiedCallPlan {
        implementation,
        args,
        markers,
    })
}

pub fn qualified_call_plan(
    namespace: &str,
    name: &str,
    positional_count: usize,
    named: &[&str],
) -> Option<QualifiedCallPlan> {
    use QualifiedArgSource::{Named as N, Positional as P};

    match (namespace, name) {
        ("net", "accept") => match (positional_count, named) {
            (2, []) => plan("tcpAcceptWithTimeout", vec![P(0), P(1)], vec![]),
            (3, []) => plan("tcpAcceptMany", vec![P(0), P(1), P(2)], vec![]),
            (4, []) => plan(
                "tcpAcceptManyWithTimeout",
                vec![P(0), P(1), P(2), P(3)],
                vec![],
            ),
            (1, names) if has_named(names, &["wait"]) => {
                plan("tcpAcceptWithTimeout", vec![P(0), N("wait")], vec![])
            }
            (1, names) if has_named(names, &["count", "callback"]) => plan(
                "tcpAcceptMany",
                vec![P(0), N("count"), N("callback")],
                vec![],
            ),
            (1, names) if has_named(names, &["count", "wait", "callback"]) => plan(
                "tcpAcceptManyWithTimeout",
                vec![P(0), N("count"), N("wait"), N("callback")],
                vec![],
            ),
            _ => None,
        },
        ("net", "read") if positional_count == 3 => {
            let from = named.contains(&"from");
            let count = named.contains(&"count");
            let wait = named.contains(&"wait");
            let expected = from as usize + count as usize + wait as usize;
            if named.len() != expected {
                return None;
            }
            let implementation = match (from, count, wait) {
                (false, false, false) => return None,
                (false, false, true) => "receiveTextWithTimeout",
                (false, true, false) => "receiveTextMany",
                (false, true, true) => "receiveTextManyWithTimeout",
                (true, false, false) => "receiveTextFrom",
                (true, false, true) => "receiveTextFromWithTimeout",
                (true, true, false) => "receiveTextFromMany",
                (true, true, true) => "receiveTextFromManyWithTimeout",
            };
            let mut args = vec![P(0), P(1)];
            if count {
                args.push(N("count"));
            }
            if wait {
                args.push(N("wait"));
            }
            args.push(P(2));
            plan(
                implementation,
                args,
                if from { vec!["from"] } else { vec![] },
            )
        }
        ("net", "write") => {
            let parts = named.contains(&"parts");
            let at = named.contains(&"at");
            let wait = named.contains(&"wait");
            let host = named.contains(&"host");
            let port = named.contains(&"port");
            let expected =
                parts as usize + at as usize + wait as usize + host as usize + port as usize;
            if named.len() != expected {
                return None;
            }
            if host || port {
                if host && port && !at && !wait {
                    return if parts && positional_count == 1 {
                        plan(
                            "sendTextToParts",
                            vec![P(0), N("host"), N("port"), N("parts")],
                            vec![],
                        )
                    } else if !parts && positional_count == 2 {
                        plan("sendTextTo", vec![P(0), N("host"), N("port"), P(1)], vec![])
                    } else {
                        None
                    };
                }
                return None;
            }
            let value = if parts {
                if positional_count != 1 {
                    return None;
                }
                N("parts")
            } else {
                if positional_count != 2 {
                    return None;
                }
                P(1)
            };
            let mut args = vec![P(0), value];
            if at {
                args.push(N("at"));
            }
            if wait {
                args.push(N("wait"));
            }
            let implementation = match (parts, at, wait) {
                (false, false, false) => return None,
                (false, false, true) => "sendTextWithTimeout",
                (false, true, false) => "sendTextProgress",
                (false, true, true) => "sendTextProgressWithTimeout",
                (true, false, false) => "sendTextParts",
                (true, false, true) => "sendTextPartsWithTimeout",
                (true, true, false) => "sendTextPartsProgress",
                (true, true, true) => "sendTextPartsProgressWithTimeout",
            };
            plan(implementation, args, vec![])
        }
        ("net", "readable") if positional_count == 3 && named.is_empty() => {
            plan("waitReadableMany", vec![P(0), P(1), P(2)], vec![])
        }
        ("net", "writable") if positional_count == 3 && named.is_empty() => {
            plan("waitWritableMany", vec![P(0), P(1), P(2)], vec![])
        }
        ("net", "close") if positional_count == 1 && has_named(named, &["read"]) => {
            plan("shutdownRead", vec![P(0)], vec!["read"])
        }
        ("net", "close") if positional_count == 1 && has_named(named, &["write"]) => {
            plan("shutdownWrite", vec![P(0)], vec!["write"])
        }
        ("http", "read") => match (positional_count, named) {
            (4, []) => plan(
                "receiveRequestHeadWithHeaders",
                vec![P(0), P(1), P(2), P(3)],
                vec![],
            ),
            (6, []) => plan(
                "receiveRequestWithTextBody",
                vec![P(0), P(1), P(2), P(3), P(4), P(5)],
                vec![],
            ),
            (3, names) if has_named(names, &["headers"]) => plan(
                "receiveRequestHeadWithHeaders",
                vec![P(0), P(1), P(2), N("headers")],
                vec![],
            ),
            (3, names) if has_named(names, &["bodyBytes", "headers", "body"]) => plan(
                "receiveRequestWithTextBody",
                vec![P(0), P(1), N("bodyBytes"), P(2), N("headers"), N("body")],
                vec![],
            ),
            _ => None,
        },
        ("http", "response") => match (positional_count, named) {
            (6, []) => plan(
                "receiveResponseWithTextBody",
                vec![P(0), P(1), P(2), P(3), P(4), P(5)],
                vec![],
            ),
            (4, names) if has_named(names, &["bodyBytes", "body"]) => plan(
                "receiveResponseWithTextBody",
                vec![P(0), P(1), N("bodyBytes"), P(2), P(3), N("body")],
                vec![],
            ),
            _ => None,
        },
        ("http", "request") if has_named(named, &["headers"]) => match positional_count {
            6 => plan(
                "requestWithHeaders",
                vec![P(0), P(1), P(2), P(3), P(4), P(5), N("headers")],
                vec![],
            ),
            7 => plan(
                "requestWithHeaders",
                vec![P(0), P(1), P(2), P(3), P(4), P(5), N("headers"), P(6)],
                vec![],
            ),
            _ => None,
        },
        ("http", "respond") if has_named(named, &["headers"]) => match positional_count {
            4 => plan(
                "respondWithHeaders",
                vec![P(0), P(1), P(2), P(3), N("headers")],
                vec![],
            ),
            5 => plan(
                "respondWithHeaders",
                vec![P(0), P(1), P(2), P(3), N("headers"), P(4)],
                vec![],
            ),
            _ => None,
        },
        ("http", "serve") if positional_count == 6 && has_named(named, &["parallel", "limit"]) => {
            plan(
                "serveConcurrentLimit",
                vec![P(0), P(1), P(2), N("limit"), P(3), P(4), P(5)],
                vec!["parallel"],
            )
        }
        ("http", "serve") if positional_count == 6 && has_named(named, &["limit"]) => plan(
            "serveConcurrentLimit",
            vec![P(0), P(1), P(2), N("limit"), P(3), P(4), P(5)],
            vec![],
        ),
        ("http", "serve") if has_named(named, &["parallel"]) => plan(
            "serveConcurrent",
            (0..positional_count).map(P).collect(),
            vec!["parallel"],
        ),
        ("url", "encode") if has_named(named, &["form"]) => plan(
            "encodeFormComponent",
            (0..positional_count).map(P).collect(),
            vec!["form"],
        ),
        ("url", "decode") if has_named(named, &["form"]) => plan(
            "decodeFormComponent",
            (0..positional_count).map(P).collect(),
            vec!["form"],
        ),
        ("file", "mode") if positional_count == 2 && named.is_empty() => {
            plan("setPermissions", vec![P(0), P(1)], vec![])
        }
        ("file", "modified") if positional_count == 2 && named.is_empty() => {
            plan("setModified", vec![P(0), P(1)], vec![])
        }
        ("file", "accessed") if positional_count == 2 && named.is_empty() => {
            plan("setAccessed", vec![P(0), P(1)], vec![])
        }
        ("directory", "mode") if positional_count == 2 && named.is_empty() => {
            plan("setPermissions", vec![P(0), P(1)], vec![])
        }
        ("directory", "modified") if positional_count == 2 && named.is_empty() => {
            plan("setModified", vec![P(0), P(1)], vec![])
        }
        ("directory", "accessed") if positional_count == 2 && named.is_empty() => {
            plan("setAccessed", vec![P(0), P(1)], vec![])
        }
        ("worker", "start") if positional_count == 2 && named.is_empty() => {
            plan("startWith", vec![P(0), P(1)], vec![])
        }
        ("worker", "join") if positional_count == 0 && named.is_empty() => {
            plan("joinChildren", vec![], vec![])
        }
        ("worker", "cancel") if positional_count == 0 && named.is_empty() => {
            plan("cancelChildren", vec![], vec![])
        }
        ("android", "record") if positional_count == 0 && named.is_empty() => {
            plan("stopMicrophoneRecording", vec![], vec![])
        }
        ("android", "notify") if positional_count == 4 && has_named(named, &["label", "url"]) => {
            plan(
                "notifyUrlAction",
                vec![P(0), P(1), P(2), P(3), N("label"), N("url")],
                vec![],
            )
        }
        _ => None,
    }
}

pub fn global<'a>(name: &'a str) -> &'a str {
    match name {
        "filter" => "where",
        "every" => "all",
        "concat" => "merge",
        "distinct" => "unique",
        "flatten" => "flat",
        "sorted" => "sort",
        "chunked" => "chunk",
        _ => name,
    }
}

pub fn global_impl<'a>(name: &'a str) -> &'a str {
    match name {
        "all" => "every",
        "merge" => "concat",
        "unique" => "distinct",
        "flat" => "flatten",
        "sort" => "sorted",
        "chunk" => "chunked",
        _ => name,
    }
}

pub fn list_member<'a>(name: &'a str) -> &'a str {
    match name {
        "length" => "count",
        "isEmpty" => "empty",
        "isNotEmpty" => "nonempty",
        "single" => "only",
        _ => name,
    }
}

pub fn list_member_impl<'a>(name: &'a str) -> &'a str {
    match name {
        "count" => "length",
        "empty" => "isEmpty",
        "nonempty" => "isNotEmpty",
        "only" => "single",
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_short_and_legacy_names_remain_resolvable() {
        assert_eq!(qualified("android", "startMicrophoneRecording"), "record");
        assert_eq!(qualified("android", "secureRead"), "load");
        assert_eq!(qualified("net", "sendTextWithTimeout"), "write");
        assert_eq!(qualified("net", "receiveTextFromManyWithTimeout"), "read");
        assert_eq!(qualified("http", "sendTextResponseWithHeaders"), "respond");
        assert_eq!(qualified("http", "receiveRequestWithTextBody"), "read");
        assert_eq!(qualified("time", "utcDayOfYear"), "yearday");
        assert_eq!(qualified("file", "setPermissions"), "mode");
        assert_eq!(qualified("directory", "allocatedSize"), "space");
        assert_eq!(qualified("android", "showKeyboard"), "keyboard");
        assert_eq!(
            qualified_impl("android", "record"),
            "startMicrophoneRecording"
        );
        assert_eq!(qualified_impl("net", "alive"), "setKeepAlive");
        assert_eq!(qualified_impl("http", "read"), "receiveRequestHead");
        assert_eq!(global("distinct"), "unique");
        assert_eq!(global("where"), "where");
        assert_eq!(global_impl("unique"), "distinct");
        assert_eq!(global("concat"), "merge");
        assert_eq!(global_impl("merge"), "concat");
        assert_eq!(list_member("isNotEmpty"), "nonempty");
        assert_eq!(list_member_impl("only"), "single");

        let read = qualified_call_plan("net", "read", 3, &["from", "count", "wait"]).unwrap();
        assert_eq!(read.implementation, "receiveTextFromManyWithTimeout");
        let mode = qualified_call_plan("file", "mode", 2, &[]).unwrap();
        assert_eq!(mode.implementation, "setPermissions");
        let stop = qualified_call_plan("android", "record", 0, &[]).unwrap();
        assert_eq!(stop.implementation, "stopMicrophoneRecording");
        let limited = qualified_call_plan("http", "serve", 6, &["limit"]).unwrap();
        assert_eq!(limited.implementation, "serveConcurrentLimit");
    }
}
