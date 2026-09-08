view ResponsiveDemo {
    grid columns: 1fr
    grid rows: auto auto auto auto
    grid gap: 12
    grid padding: 24

    Text landscape at 1,1
        text: "Landscape layout"
        visible: window_is_landscape
        size: 24
        bold: true

    Text portrait at 2,1
        text: "Portrait layout"
        visible: window_is_portrait

    Text density at 3,1
        text: "HiDPI display"
        visible: display_scale > 1

    Button width_mode at 4,1
        text: "Responsive window"
        enabled: window_width >= 700 && window_height >= 300
}

app ResponsiveDemo(title: "Flux responsive view", width: 760, height: 420, resizable: true)
