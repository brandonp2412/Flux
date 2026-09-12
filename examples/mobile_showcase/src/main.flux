view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 104 56 104 1fr
    grid rows: auto auto auto auto auto auto auto
    grid gap: 8
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 5
        text: "♥︎  HORSE TINDER"
        color: "#FF6077"
        size: 24
        bold: true
        wrap: false
        letterSpacing: 1

    Text nearby at 1,1 span columns 5
        text: "4 KM  ·  ONLINE"
        color: "#AEB5C0"
        size: 12
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "right"
        alignX: "end"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 300
        maxHeight: 312
        backgroundColor: "#13151B"
        borderColor: "#303743"
        borderWidth: 1
        radius: 28
        shadowColor: "#00000052"
        shadowBlur: 4
        shadowOffsetY: 2

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "text"
        size: 35
        bold: true
        wrap: false

    Text matchBadge at 3,1 span columns 5
        text: "98% MATCH"
        color: "#FFA2AF"
        size: 12
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 9
        backgroundColor: "#461923"
        borderColor: "#984052"
        borderWidth: 1
        radius: 999

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#C5CAD3"
        size: 15
        wrap: false

    Text weekend at 5,1 span columns 5
        text: "BEACH GALLOPS"
        color: "#D2D6DF"
        size: 13
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "start"
        padding: 11
        backgroundColor: "#191E26"
        borderColor: "#343B48"
        borderWidth: 1
        radius: 999

    Text greenFlag at 5,1 span columns 5
        text: "SHARES HAY"
        color: "#C9F0DA"
        size: 13
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        padding: 11
        backgroundColor: "#14261C"
        borderColor: "#345B40"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#F0F1F5"
        size: 16
        wrap: true
        maxLines: 3
        lineHeightPercent: 140
        padding: 13
        backgroundColor: "#11151C"
        borderColor: "#303845"
        borderWidth: 1
        radius: 20
        shadowColor: "#00000030"
        shadowBlur: 2
        shadowOffsetY: 1

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 64
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 104
        maxWidth: 104
        minHeight: 104
        alignX: "center"
        translateX: -84
        translateY: -8
        focusable: true
        padding: 0
        radius: 999
        backgroundColor: "#171C24"
        borderColor: "#4E5967"
        borderWidth: 1
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 72
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 104
        maxWidth: 104
        minHeight: 104
        alignX: "center"
        translateX: 84
        translateY: -8
        focusable: true
        padding: 0
        radius: 999
        backgroundColor: "#FF4962"
        borderColor: "#FF8998"
        borderWidth: 1
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
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

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#07080B", surfaceRaisedColor: "#11151C", textColor: "#F7F8FA", textMutedColor: "#9298A4", accentColor: "#FF4962", onAccentColor: "#FFFFFF", outlineColor: "#2B323D", shadowColor: "#00000080")
