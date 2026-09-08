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
        align_x: "center"
        background_color: "#2563EB"
        border_color: "#1E40AF"
        border_width: 2
        radius: 12
        margin: 16
        translate_x: offset
        rotate_degrees: offset
        scale_percent: 100 + offset
        scale_y_percent: 100 - offset
        skew_x_degrees: offset
        transform_origin_x_percent: 50 + offset
        transition_ms: 180
        transition_easing: "ease_out"

    Button move at 2,1
        text: "Move"
        primary: true
        align_x: "center"
        on_press: offset => offset + 4
}

app TransformDemo(title: "Flux transforms", width: 640, height: 360)
