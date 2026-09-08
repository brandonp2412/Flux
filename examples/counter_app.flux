view CounterApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    grid padding: 24
    state count: i64 = 0

    Text status at 1,1
        text: "Count is positive"
        visible: count > 0
        size: 28
        bold: true

    Button increment at 2,1
        text: "Increment"
        enabled: count >= 0
        on_press: count => count + 1
}

app CounterApp
