fn started() -> void {
    android.vibrate(25)
    print(android.permission_granted("android.permission.VIBRATE"))
    android.create_notification_channel("updates", "Flux updates", "Native Flux Android notifications")
    if android.notification_permission_granted():
        android.notify_url_action("updates", 1, "Flux", "Native Android notifications work", "Open", "https://example.com")
    else:
        android.request_notification_permission()
    print("Flux Android started")
}

fn configuration_changed() -> void {
    print("Flux Android configuration changed")
}

fn low_memory() -> void {
    print("Flux Android low memory")
}

fn save_state() -> str {
    return "flux-android-example"
}

fn restore_state(value: str) -> void {
    print(value)
}

fn pressed() -> void {
    print("Flux Android button pressed")
}

fn text_changed(value: str) -> void {
    print(value)
}

view Screen {
    state expanded: bool = false
    state choice: i64 = 0
    derived actionLabel: str = "Toggle details"
    grid columns: 1fr
    grid rows: auto auto auto auto auto auto auto auto
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
        onChange: text_changed
        onSubmit: text_changed
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
}

app Screen(on_start: started, on_configuration_changed: configuration_changed, on_low_memory: low_memory, on_save_state: save_state, on_restore_state: restore_state)
