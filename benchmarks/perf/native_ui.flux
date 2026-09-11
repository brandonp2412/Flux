view NativeUiBench {
    grid columns: 1fr
    grid rows: auto auto auto
    grid gap: 12
    grid padding: 24

    Text title at 1,1
        text: "Flux Native UI"
        size: 28
        bold: true

    Text status at 2,1
        text: "Ready"

    Button action at 3,1
        text: "Toggle"
        primary: true
}

app NativeUiBench(id: "app.flux.bench.nativeui", title: "Flux Native UI Benchmark", width: 420, height: 260, resizable: true)
