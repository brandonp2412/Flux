fn app_started() -> void {
    print("Flux app started")
}

fn app_exiting() -> void {
    print("Flux app exiting")
}

view HelloApp {
    grid columns: 1fr
    grid rows: auto auto auto
    grid gap: 12
    grid padding: 24
    state clicked: bool = false

    Text title at 1,1
        text: "Hello, Flux!"
        size: 28
        bold: true
        color: "#4F46E5"
        visible: !clicked

    Text status at 2,1
        text: "Clicked!"
        visible: clicked

    Button action at 3,1
        text: "Toggle"
        primary: true
        shortcut: "Ctrl+Enter"
        on_press: clicked => !clicked
}

app HelloApp(id: "app.flux.hello", title: "Flux Hello", width: 420, height: 260, resizable: true, on_start: app_started, on_exit: app_exiting)
