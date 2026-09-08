fn clicked() -> void {
    print("Clicked from Flux")
}

view HelloApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12

    Text title at 1,1
        text: "Hello, Flux!"

    Button action at 2,1
        text: "Click me"
        on_press: clicked
}

app HelloApp
