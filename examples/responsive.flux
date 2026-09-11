view ResponsiveDemo {
    grid columns: 1fr
    grid rows: auto auto auto auto
    grid gap: 12
    grid padding: 24

    Text landscape at 1,1
        text: "Landscape layout"
        visible: windowIsLandscape
        size: 24
        bold: true

    Text portrait at 2,1
        text: "Portrait layout"
        visible: windowIsPortrait

    Text density at 3,1
        text: "HiDPI display"
        visible: displayScale > 1

    Button widthMode at 4,1
        text: "Medium window"
        enabled: windowIsMedium && windowHeight >= 300
}

app ResponsiveDemo(title: "Flux responsive view", width: 760, height: 420, resizable: true)
