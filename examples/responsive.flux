view ResponsiveDemo {
    grid columns: 1fr
    grid rows: auto auto auto
    grid gap: 12
    grid padding: 24

    Text orientation at 1,1
        text: "Landscape layout" if window_is_landscape else "Portrait layout"
        size: 24
        bold: true

    Text density at 2,1
        text: "HiDPI display" if display_scale > 1 else "Standard density"

    Button width_mode at 3,1
        text: "Wide window" if window_width >= 700 else "Compact window"
        enabled: window_height >= 300
}

app ResponsiveDemo(title: "Flux responsive view", width: 760, height: 420, resizable: true)
