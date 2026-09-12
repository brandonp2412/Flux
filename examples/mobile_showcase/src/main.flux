view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 90 28 90 1fr
    grid rows: auto auto auto auto auto auto auto
    grid gap: 10
    grid padding: 22
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 4
        text: "♥︎  HORSE TINDER"
        color: "#FF4458"
        size: 18
        bold: true
        letterSpacing: 1

    Text nearby at 1,5
        text: "4 KM  ·  ONLINE"
        color: "#9CA1AC"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "right"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 415
        backgroundColor: "#13151B"
        borderColor: "#303540"
        borderWidth: 1
        radius: 28
        shadowColor: "#00000080"
        shadowBlur: 14
        shadowOffsetY: 5

    Text profileName at 3,1 span columns 4
        text: "BUTTERCUP, 7"
        color: "text"
        size: 31
        bold: true

    Text matchBadge at 3,5
        text: "98% MATCH"
        color: "#FF8291"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "right"
        alignY: "center"

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#ADB2BD"
        size: 15

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
        color: "#E1E4EA"
        size: 16
        wrap: true
        maxLines: 3
        lineHeightPercent: 142
        padding: 18
        backgroundColor: "#11141B"
        borderColor: "#272C37"
        borderWidth: 1
        radius: 22

    Button pass at 7,1 span columns 5
        text: "✕"
        size: 44
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 96
        maxWidth: 96
        minHeight: 96
        alignX: "center"
        focusable: true
        padding: 0
        radius: 48
        translateX: -62
        backgroundColor: "#171A21"
        borderColor: "#3A404C"
        borderWidth: 1
        shadowColor: "#00000078"
        shadowBlur: 8
        shadowOffsetY: 3
        onPress: passed => !passed

    Button like at 7,1 span columns 5
        text: "♥︎"
        size: 46
        accessibilityLabel: "Like Buttercup"
        minWidth: 96
        maxWidth: 96
        minHeight: 96
        alignX: "center"
        focusable: true
        primary: true
        padding: 0
        radius: 48
        translateX: 62
        backgroundColor: "#FF4458"
        borderColor: "#FF6374"
        borderWidth: 1
        shadowColor: "#00000088"
        shadowBlur: 9
        shadowOffsetY: 3
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
