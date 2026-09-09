fn started() -> void {
    print("Flux Android started")
}

view Screen {
    grid columns: 1fr
    grid rows: auto
    Text title at 1,1
        text: "Flux on Android"
}

app Screen(on_start: started)
