view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 104 36 104 1fr
    grid rows: auto auto auto auto auto auto auto
    grid gap: 12
    grid padding: 24
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "♥︎  HORSE TINDER"
        color: "#FF5268"
        size: 21
        bold: true
        letterSpacing: 1

    Text nearby at 1,4 span columns 2
        text: "4 KM  ·  ONLINE"
        color: "#B1B6C0"
        size: 12
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
        borderColor: "#292E38"
        borderWidth: 1
        radius: 30
        shadowColor: "#00000070"
        shadowBlur: 14
        shadowOffsetY: 5

    Text profileName at 3,1 span columns 3
        text: "Buttercup, 7"
        color: "text"
        size: 32
        bold: true

    Text matchBadge at 3,4 span columns 2
        text: "98% MATCH"
        color: "#FF93A0"
        size: 12
        bold: true
        letterSpacing: 1
        textAlign: "center"
        alignY: "center"
        padding: 9
        backgroundColor: "#2A151B"
        borderColor: "#61303B"
        borderWidth: 1
        radius: 999

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#B5BAC4"
        size: 15

    Text weekend at 5,1 span columns 2
        text: "BEACH GALLOPS"
        color: "#C5CAD5"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        padding: 14
        backgroundColor: "#181C24"
        borderColor: "#2A303B"
        borderWidth: 1
        radius: 999

    Text greenFlag at 5,4 span columns 2
        text: "SHARES HAY"
        color: "#BFEBD2"
        size: 13
        bold: true
        letterSpacing: 1
        textAlign: "center"
        padding: 14
        backgroundColor: "#14231B"
        borderColor: "#294332"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 5
        text: "Emotionally available. Great listener. Her best friend refuses to stay out of profile photos."
        color: "#E8EAF0"
        size: 17
        wrap: true
        maxLines: 3
        lineHeightPercent: 145
        padding: 18
        backgroundColor: "#11141A"
        borderColor: "#272D38"
        borderWidth: 1
        radius: 20

    Button pass at 7,2
        text: "✕"
        size: 50
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 100
        maxWidth: 100
        minHeight: 100
        alignX: "center"
        focusable: true
        padding: 0
        radius: 50
        backgroundColor: "#171B23"
        borderColor: "#3F4653"
        borderWidth: 2
        shadowColor: "#00000000"
        shadowBlur: 0
        shadowOffsetY: 0
        onPress: passed => !passed

    Button like at 7,4
        text: "♥︎"
        size: 54
        accessibilityLabel: "Like Buttercup"
        minWidth: 100
        maxWidth: 100
        minHeight: 100
        alignX: "center"
        focusable: true
        primary: true
        padding: 0
        radius: 50
        backgroundColor: "#FF4458"
        borderColor: "#FF7181"
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
