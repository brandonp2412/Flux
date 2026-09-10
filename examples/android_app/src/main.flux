fn started() -> void {
    android.vibrate(25)
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

view Screen {
    grid columns: 1fr
    grid rows: auto auto
    Text title at 1,1
        text: "Flux on Android"
    Button action at 2,1
        text: "Press from Flux"
        onPress: pressed
}

app Screen(on_start: started, on_configuration_changed: configuration_changed, on_low_memory: low_memory, on_save_state: save_state, on_restore_state: restore_state)
