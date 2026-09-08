view HelloApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    grid padding: 24
    state clicked: bool = false

    Text title at 1,1
        text: "Clicked!" if clicked else "Hello, Flux!"
        size: 28
        bold: true
        color: "#4F46E5"

    Button action at 2,1
        text: "Reset" if clicked else "Click me"
        primary: true
        on_press: clicked => !clicked
}

app HelloApp
