view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 104 56 104 1fr
    grid rows: auto 368 auto auto auto auto auto
    grid gap: 9
    grid padding: 20
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
        color: "#C8CDD5"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        padding: 8
        backgroundColor: "#151A21"
        borderColor: "#303844"
        borderWidth: 1
        radius: 18

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 280
        maxHeight: 292
        backgroundColor: "#12151B"
        borderColor: "#29313C"
        borderWidth: 1
        radius: 32
        shadowColor: "#00000066"
        shadowBlur: 6
        shadowOffsetY: 3

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "text"
        size: 37
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
        color: "#BFC5CF"
        size: 14
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
        padding: 12
        backgroundColor: "#171C24"
        borderColor: "#343D49"
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
        padding: 12
        backgroundColor: "#13251B"
        borderColor: "#31583E"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#F0F1F5"
        size: 16
        wrap: true
        maxLines: 3
        lineHeightPercent: 140
        padding: 14
        backgroundColor: "#10141A"
        borderColor: "#29323D"
        borderWidth: 1
        radius: 24
        shadowColor: "#00000040"
        shadowBlur: 3
        shadowOffsetY: 2

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 68
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 106
        maxWidth: 106
        minHeight: 106
        alignX: "center"
        translateX: -86
        translateY: -8
        focusable: true
        padding: 0
        radius: 53
        backgroundColor: "#151A22"
        borderColor: "#394452"
        borderWidth: 2
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 76
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 106
        maxWidth: 106
        minHeight: 106
        alignX: "center"
        translateX: 86
        translateY: -8
        focusable: true
        padding: 0
        radius: 53
        backgroundColor: "#FF4D67"
        borderColor: "#FF91A0"
        borderWidth: 2
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
