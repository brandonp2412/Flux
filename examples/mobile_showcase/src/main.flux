view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 1fr
    grid rows: auto auto auto auto auto auto auto auto auto
    grid gap: 12
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1
        text: "♥︎  HORSE TINDER"
        color: "#E94057"
        size: 18
        bold: true
        letterSpacing: 1

    Text nearby at 1,2
        text: "NEARBY  ·  4 KM"
        color: "textMuted"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "right"

    Text profileCard at 2,1 span columns 2
        text: ""
        minHeight: 360
        backgroundColor: "#FCE7EE"
        borderColor: "#F8B7C8"
        borderWidth: 1
        radius: 32
        shadowColor: "#7A243A22"
        shadowBlur: 24
        shadowOffsetY: 10

    Text horse at 2,1 span columns 2
        text: "🐴"
        size: 84
        textAlign: "center"
        alignY: "center"
        accessibilityHidden: true
        translateY: -72

    Text profileName at 2,1 span columns 2
        text: "BUTTERCUP, 7"
        color: "#351B24"
        size: 30
        bold: true
        textAlign: "center"
        alignY: "center"
        translateY: 58

    Text profileMeta at 2,1 span columns 2
        text: "Palomino  ·  16.1 hands"
        color: "#6D3B4B"
        size: 17
        bold: true
        textAlign: "center"
        alignY: "center"
        translateY: 104

    Text chemistry at 3,1 span columns 2
        text: "✓  98% mane chemistry    ·    carrot verified"
        color: "#B4234C"
        size: 14
        bold: true
        textAlign: "center"
        padding: 10
        backgroundColor: "#FFF0F4"
        radius: 999

    Text weekend at 4,1
        text: "WEEKEND\nBeach gallops\n+ snacks"
        color: "#25314A"
        size: 15
        bold: true
        wrap: true
        padding: 14
        minHeight: 86
        backgroundColor: "#F3F5F9"
        radius: 18

    Text greenFlag at 4,2
        text: "GREEN FLAG\nShares hay.\nNo drama."
        color: "#1E5539"
        size: 15
        bold: true
        wrap: true
        padding: 14
        minHeight: 86
        backgroundColor: "#EEF8F2"
        radius: 18

    Text bio at 5,1 span columns 2
        text: "Emotionally available. Great listener. Won't stop bringing up the one tiny fence she jumped in 2024."
        color: "text"
        size: 16
        wrap: true
        maxLines: 4
        padding: 16
        backgroundColor: "surfaceRaised"
        borderColor: "outline"
        borderWidth: 1
        radius: 20

    Button pass at 6,1
        text: "✕"
        accessibilityLabel: "Pass on Buttercup"
        minHeight: 62
        padding: 14
        radius: 999
        backgroundColor: "#FFFFFF"
        borderColor: "#E4D8DC"
        borderWidth: 1
        shadowColor: "#351B2418"
        shadowBlur: 10
        shadowOffsetY: 4
        onPress: passed => !passed

    Button like at 6,2
        text: "♥︎"
        accessibilityLabel: "Like Buttercup"
        minHeight: 62
        primary: true
        padding: 14
        radius: 999
        shadowColor: "#E9405740"
        shadowBlur: 12
        shadowOffsetY: 5
        onPress: liked => !liked

    Text result at 7,1 span columns 2
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#A51F46"
        size: 14
        bold: true
        textAlign: "center"
        padding: 14
        backgroundColor: "#FFF0F4"
        radius: 18
        layoutTransitionMs: motionNormal

    Text passedNote at 8,1 span columns 2
        text: "Passed. Buttercup will pretend not to care."
        visible: passed
        color: "textMuted"
        size: 14
        textAlign: "center"
        padding: 14
        backgroundColor: "#F5F5F6"
        radius: 18
        layoutTransitionMs: motionNormal

    Text footer at 9,1 span columns 2
        text: "No endless swiping. Just stable relationships."
        color: "textMuted"
        size: 12
        textAlign: "center"
        marginTop: 2
        marginBottom: 10
}

app HorseTinder(title: "Horse Tinder", theme: "light", surfaceColor: "#FFF9FB", surfaceRaisedColor: "#FFFFFF", textColor: "#22191C", textMutedColor: "#776B70", accentColor: "#E94057", onAccentColor: "#FFFFFF", outlineColor: "#E7DDE1", shadowColor: "#351B2424")
