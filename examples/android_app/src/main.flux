fn started() -> void {
    android.vibrate(25)
    android.create_notification_channel("updates", "Flux updates", "Native Flux Android notifications")
    if android.notification_permission_granted():
        android.notify_url_action("updates", 1, "Flux", "Native Android notifications work", "Open", "https://example.com")
    else:
        android.request_notification_permission()
    print("Flux Android started")
}

view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux on Android"
}

app Screen(on_start: started)
