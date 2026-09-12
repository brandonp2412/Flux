view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 96 60 96 1fr
    grid rows: auto 368 auto auto auto auto auto
    grid gap: 11
    grid padding: 20
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 5
        text: "♥︎  HORSE TINDER"
        color: "#FF6077"
        size: 23
        bold: true
        wrap: false
        letterSpacing: 1

    Text nearby at 1,1 span columns 5
        text: "4 KM  ·  ONLINE"
        color: "#D5D9E0"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        padding: 9
        backgroundColor: "#141922"
        borderColor: "#323B48"
        borderWidth: 1
        radius: 18

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 286
        maxHeight: 300
        backgroundColor: "#11151B"
        borderColor: "#303844"
        borderWidth: 1
        radius: 30
        shadowColor: "#00000070"
        shadowBlur: 5
        shadowOffsetY: 3

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "text"
        size: 35
        bold: true
        wrap: false

    Text matchBadge at 3,1 span columns 5
        text: "98% MATCH"
        color: "#FFB2BD"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#431822"
        borderColor: "#914052"
        borderWidth: 1
        radius: 999

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#C5CAD3"
        size: 13
        wrap: false

    Text weekend at 5,1 span columns 5
        text: "BEACH GALLOPS"
        color: "#D2D6DF"
        size: 12
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "start"
        padding: 10
        backgroundColor: "#171D26"
        borderColor: "#343D49"
        borderWidth: 1
        radius: 999

    Text greenFlag at 5,1 span columns 5
        text: "SHARES HAY"
        color: "#C9F0DA"
        size: 12
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        padding: 10
        backgroundColor: "#13271C"
        borderColor: "#31583E"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#F0F1F5"
        size: 15
        wrap: true
        maxLines: 3
        lineHeightPercent: 138
        padding: 16
        backgroundColor: "#11161D"
        borderColor: "#2D3743"
        borderWidth: 1
        radius: 22
        shadowColor: "#00000038"
        shadowBlur: 2
        shadowOffsetY: 1

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 58
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 96
        maxWidth: 96
        minHeight: 96
        alignX: "center"
        translateX: -80
        translateY: -4
        focusable: true
        padding: 0
        radius: 48
        backgroundColor: "#151B23"
        borderColor: "#3A4655"
        borderWidth: 2
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        transitionMs: motionFast
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 62
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 96
        maxWidth: 96
        minHeight: 96
        alignX: "center"
        translateX: 80
        translateY: -4
        focusable: true
        padding: 0
        radius: 48
        backgroundColor: "#FF4D67"
        borderColor: "#FF91A0"
        borderWidth: 2
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        transitionMs: motionFast
        onPress: liked => !liked

    Text result at 2,1 span columns 5
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#FFD9DE"
        size: 14
        bold: true
        textAlign: "center"
        alignX: "center"
        alignY: "end"
        margin: 20
        padding: 14
        backgroundColor: "#35141AEF"
        borderColor: "#7D2A38"
        borderWidth: 1
        radius: 20
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        layoutTransitionMs: motionNormal

    Text passedNote at 2,1 span columns 5
        text: "Passed. The photobomber is taking it personally."
        visible: passed
        color: "#D9DCE3"
        size: 13
        textAlign: "center"
        alignX: "center"
        alignY: "end"
        margin: 20
        padding: 14
        backgroundColor: "#101218E8"
        borderColor: "#2A2E38"
        borderWidth: 1
        radius: 20
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#080A0E", surfaceRaisedColor: "#11161D", textColor: "#F7F8FA", textMutedColor: "#9AA1AD", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#303945", shadowColor: "#00000080")
