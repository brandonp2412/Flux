view CounterApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    grid padding: 24
    state count: i64 = 0

    Text status at 1,1
        text: "Count is positive" if count > 0 else "Count is zero"
        size: 28
        bold: true

    Button increment at 2,1
        text: "Increment again" if count > 0 else "Increment"
        on_press: count => count + 1
}

app CounterApp
