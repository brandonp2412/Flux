fn started() -> void {
    android.vibrate(25)
    print(android.permissionGranted("android.permission.VIBRATE"))
    android.createNotificationChannel("updates", "Flux updates", "Native Flux Android notifications")
    if android.notificationPermissionGranted():
        android.notifyUrlAction("updates", 1, "Flux", "Native Android notifications work", "Open", "https://example.com")
    else:
        android.requestNotificationPermission()
    print("Flux Android started")
}

fn configurationChanged() -> void {
    print("Flux Android configuration changed")
}

fn lowMemory() -> void {
    print("Flux Android low memory")
}

fn saveState() -> str {
    return "flux-android-example"
}

fn restoreState(value: str) -> void {
    print(value)
}

fn pressed() -> void {
    print("Flux Android button pressed")
}

fn textChanged(value: str) -> void {
    print(value)
}

fn focusNextWrapped() -> void {
    android.focusNext(true)
}

fn focusLast() -> void {
    android.focusLast()
}

view Screen {
    state expanded: bool = false
    state choice: i64 = 0
    derived actionLabel: str = "Toggle details"
    grid columns: 1fr
    grid rows: auto auto auto auto auto auto auto auto auto auto
    grid gap: 12
    grid padding: 20
    Text title at 1,1
        text: "Flux on Android"
        color: "#2563EB"
        size: 24
        bold: true
        padding: 8
        backgroundColor: "#EAF2FFFF"
        radius: 10
    Text detail at 2,1
        text: "Native Flux state updated this Android view"
        visible: expanded
    Button toggle at 3,1
        text: actionLabel
        onPress: expanded => !expanded
    Button action at 4,1
        text: "Call Flux function"
        enabled: !expanded
        onPress: pressed
    TextInput query at 5,1
        placeholder: "Type in native Android"
        multiline: true
        submitOnEnter: true
        maxLength: 48
        padding: 10
        backgroundColor: "#F8FAFC"
        borderColor: "#94A3B8"
        borderWidth: 1
        borderStyle: "solid"
        radius: 8
        tooltip: "Native Android text input"
        accessibilityLabel: "Flux text input"
        accessibilityDescription: "Type text to exercise native Android input callbacks"
        onChange: textChanged
        onSubmit: textChanged
    Toggle details at 6,1
        label: "Show details"
        checked: expanded
        onChange: expanded => !expanded
    Radio first at 7,1
        label: "First choice"
        selected: choice == 0
        onSelect: choice => 0
    Radio second at 8,1
        label: "Second choice"
        selected: choice == 1
        onSelect: choice => 1
    Button focusStart at 9,1
        text: "Focus next (wrap)"
        onPress: focusNextWrapped
    Button focusEnd at 10,1
        text: "Focus last control"
        onPress: focusLast
}

app Screen(onStart: started, onConfigurationChanged: configurationChanged, onLowMemory: lowMemory, onSaveState: saveState, onRestoreState: restoreState)
