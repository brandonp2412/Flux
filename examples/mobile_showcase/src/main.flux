view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 86 28 86 1fr
    grid rows: auto auto auto auto auto auto auto auto
    grid gap: 10
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 4
        text: "♥︎  HORSE TINDER"
        color: "#FF4458"
        size: 17
        bold: true
        letterSpacing: 1

    Text nearby at 1,5
        text: "4 KM  ·  ONLINE"
        color: "textMuted"
        size: 11
        bold: true
        letterSpacing: 1
        textAlign: "right"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 420
        backgroundColor: "#13151B"
        borderColor: "#262A34"
        borderWidth: 1
        radius: 30
        shadowColor: "#00000066"
        shadowBlur: 14
        shadowOffsetY: 5

    Text profileName at 3,1 span columns 4
        text: "BUTTERCUP, 7"
        color: "text"
        size: 29
        bold: true

    Text matchBadge at 3,5
        text: "98% MATCH"
        color: "#FF7182"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "right"
        alignY: "center"

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "textMuted"
        size: 14

    Text weekend at 5,1 span columns 2
        text: "BEACH GALLOPS"
        color: "#C5CAD5"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        padding: 13
        backgroundColor: "#171A21"
        borderColor: "#242832"
        borderWidth: 1
        radius: 999

    Text greenFlag at 5,4 span columns 2
        text: "SHARES HAY"
        color: "#BFEBD2"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        padding: 13
        backgroundColor: "#142019"
        borderColor: "#23362A"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#D9DCE3"
        size: 15
        wrap: true
        maxLines: 3
        padding: 16
        backgroundColor: "#101218"
        borderColor: "#20232C"
        borderWidth: 1
        radius: 20

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 30
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 86
        maxWidth: 86
        minHeight: 86
        alignX: "center"
        padding: 0
        radius: 999
        translateX: -57
        backgroundColor: "#15181F"
        borderColor: "#3A404C"
        borderWidth: 2
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 32
        accessibilityLabel: "Like Buttercup"
        minWidth: 86
        maxWidth: 86
        minHeight: 86
        alignX: "center"
        primary: true
        padding: 0
        radius: 999
        translateX: 57
        borderColor: "#FF7182"
        borderWidth: 2
        onPress: liked => !liked

    Text result at 8,1 span columns 5
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#FFD9DE"
        size: 14
        bold: true
        textAlign: "center"
        padding: 14
        backgroundColor: "#35141A"
        borderColor: "#66212C"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal

    Text passedNote at 8,1 span columns 5
        text: "Passed. The photobomber is taking it personally."
        visible: passed
        color: "textMuted"
        size: 13
        textAlign: "center"
        padding: 14
        backgroundColor: "#101218"
        borderColor: "#20232C"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", theme: "dark", surfaceColor: "#090A0D", surfaceRaisedColor: "#111319", textColor: "#F6F7F9", textMutedColor: "#8B909C", accentColor: "#FF4458", onAccentColor: "#FFFFFF", outlineColor: "#242832", shadowColor: "#00000080")
