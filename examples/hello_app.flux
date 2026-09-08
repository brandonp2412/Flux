view HelloApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    state clicked: bool = false

    Text title at 1,1
        text: "Clicked!" if clicked else "Hello, Flux!"

    Button action at 2,1
        text: "Reset" if clicked else "Click me"
        on_press: clicked => !clicked
}

app HelloApp
