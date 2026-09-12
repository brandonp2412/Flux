view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 104 52 104 1fr
    grid rows: auto auto auto auto auto auto auto
    grid gap: 9
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 5
        text: "♥︎  HORSE TINDER"
        color: "#FF5A70"
        size: 23
        bold: true
        letterSpacing: 1

    Text nearby at 1,1 span columns 5
        text: "4 KM  ·  ONLINE"
        color: "#A7ADB8"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "right"
        alignX: "end"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 326
        backgroundColor: "#13151B"
        borderColor: "#303642"
        borderWidth: 1
        radius: 28
        shadowColor: "#00000066"
        shadowBlur: 10
        shadowOffsetY: 4

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "text"
        size: 32
        bold: true

    Text matchBadge at 3,1 span columns 5
        text: "98% MATCH"
        color: "#FF93A0"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#3D1722"
        borderColor: "#793746"
        borderWidth: 1
        radius: 999

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#B8BDC7"
        size: 14

    Text weekend at 5,1 span columns 5
        text: "BEACH GALLOPS"
        color: "#D2D6DF"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        alignX: "start"
        padding: 12
        backgroundColor: "#1A1E26"
        borderColor: "#343B48"
        borderWidth: 1
        radius: 999

    Text greenFlag at 5,1 span columns 5
        text: "SHARES HAY"
        color: "#C9F0DA"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        padding: 12
        backgroundColor: "#15271D"
        borderColor: "#345B40"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#ECEEF3"
        size: 16
        wrap: true
        maxLines: 3
        lineHeightPercent: 142
        padding: 16
        backgroundColor: "#12161D"
        borderColor: "#303846"
        borderWidth: 1
        radius: 18
        shadowColor: "#00000044"
        shadowBlur: 5
        shadowOffsetY: 2

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 54
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 104
        maxWidth: 104
        minHeight: 104
        alignX: "center"
        translateX: -78
        focusable: true
        padding: 0
        radius: 52
        backgroundColor: "#181C24"
        borderColor: "#414956"
        borderWidth: 1
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 60
        accessibilityLabel: "Like Buttercup"
        minWidth: 104
        maxWidth: 104
        minHeight: 104
        alignX: "center"
        translateX: 78
        focusable: true
        primary: true
        padding: 0
        radius: 52
        backgroundColor: "#FF4458"
        borderColor: "#FF7585"
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
        margin: 18
        padding: 14
        backgroundColor: "#35141AEF"
        borderColor: "#7D2A38"
        borderWidth: 1
        radius: 18
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
        margin: 18
        padding: 14
        backgroundColor: "#101218E8"
        borderColor: "#2A2E38"
        borderWidth: 1
        radius: 18
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", theme: "dark", surfaceColor: "#090A0D", surfaceRaisedColor: "#111319", textColor: "#F6F7F9", textMutedColor: "#8B909C", accentColor: "#FF4458", onAccentColor: "#FFFFFF", outlineColor: "#242832", shadowColor: "#00000080")
