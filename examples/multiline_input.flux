fn submitted(value: str) -> void {
    print(value)
}

view Notes {
    grid columns: 1fr
    grid rows: auto auto
    Text heading at 1,1
        text: "Notes"
        variant: "title"
    TextInput editor at 2,1
        text: "Write across\nmultiple lines"
        multiline: true
        keyboardType: "text"
        onSubmit: submitted
}

app Notes(title: "Flux multiline input", width: 520, height: 360)
