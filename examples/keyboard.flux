fn keyPressed(key: str) -> void {
    print(key)
}

view KeyboardDemo {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    grid padding: 24

    Text heading at 1,1
        text: "Focus me and press a key"
        variant: "heading"
        onKey: keyPressed

    Text hint at 2,1
        text: "Special keys use portable Flux names"
        variant: "caption"
}

app KeyboardDemo(id: "app.flux.keyboard", title: "Flux Keyboard", width: 480, height: 220)
