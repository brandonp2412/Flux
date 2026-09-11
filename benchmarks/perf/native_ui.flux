view NativeUiBench {
    grid columns: 1fr
    grid rows: auto auto auto auto
    grid gap: 12
    grid padding: 24
    state toggled: bool = false

    Text title at 1,1
        text: "Flux Native UI"
        size: 28
        bold: true

    Text ready at 2,1
        text: "Ready"
        visible: !toggled

    Text toggledStatus at 3,1
        text: "Toggled"
        visible: toggled

    Button action at 4,1
        text: "Toggle"
        primary: true
        onPress: toggled => !toggled
}

app NativeUiBench(id: "app.flux.bench.nativeui", title: "Flux Native UI Benchmark", width: 420, height: 260, resizable: true)
