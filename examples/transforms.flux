view TransformDemo {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 16
    grid padding: 32
    state offset: i64 = 0

    Text card at 1,1
        text: "Native Flux transform"
        size: 24
        bold: true
        alignX: "center"
        backgroundColor: "#2563EB"
        borderColor: "#1E40AF"
        borderWidth: 2
        radius: 12
        margin: 16
        translateX: offset
        rotateDegrees: offset
        scalePercent: 100 + offset
        scaleYPercent: 100 - offset
        skewXDegrees: offset
        transformOriginXPercent: 50 + offset
        transitionMs: 180
        transitionEasing: "easeOut"

    Button move at 2,1
        text: "Move"
        primary: true
        alignX: "center"
        onPress: offset => offset + 4
}

app TransformDemo(title: "Flux transforms", width: 640, height: 360)
