view CounterApp {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 12
    grid padding: 24
    state count: i64 = 0
    derived positive: bool = count > 0
    derived canIncrement: bool = count < 10

    Text status at 1,1
        text: "Count is positive"
        visible: positive
        size: 28
        bold: true

    Button increment at 2,1
        text: "Increment"
        enabled: canIncrement
        onPress: count => count + 1
}

app CounterApp
