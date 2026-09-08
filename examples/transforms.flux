view TransformDemo {
    grid columns: 1fr
    grid rows: auto auto
    grid gap: 16
    grid padding: 32
    state active: bool = false

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
        translate_x: 48 if active else 0
        rotate_degrees: 8 if active else 0
        scale_percent: 110 if active else 100
        scale_y_percent: 92 if active else 100
        skew_x_degrees: 4 if active else 0
        transform_origin_x_percent: 25 if active else 50
        transition_ms: 180
        transition_easing: "ease_out"

    Button toggle at 2,1
        text: "Reset" if active else "Transform"
        primary: true
        align_x: "center"
        on_press: active => !active
}

app TransformDemo(title: "Flux transforms", width: 640, height: 360)
