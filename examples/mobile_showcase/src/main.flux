view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 68 14 72
    grid rows: auto 520 auto auto auto auto auto
    grid gap: 8
    grid padding: 14
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 4
        text: "♥︎  HORSE TINDER"
        color: "#FF5A73"
        size: 22
        bold: true
        wrap: false
        letterSpacing: 1

    Text nearby at 1,1 span columns 4
        text: "ONLINE  ·  4 KM"
        color: "#B9C0CB"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        alignX: "end"
        alignY: "center"

    Image profilePhoto at 2,1 span columns 4
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 500
        maxHeight: 520
        backgroundColor: "#11151B"
        borderColor: "#252C36"
        borderWidth: 1
        radius: 32
        shadowColor: "#00000080"
        shadowBlur: 8
        shadowOffsetY: 5

    Text profileName at 3,1
        text: "Buttercup, 7"
        color: "#FFFFFF"
        size: 33
        bold: true
        wrap: false
        alignY: "center"

    Button pass at 3,2
        text: "✕"
        size: 41
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 64
        maxWidth: 64
        minHeight: 64
        alignX: "center"
        translateY: -43
        focusable: true
        padding: 0
        radius: 32
        backgroundColor: "#151B23"
        borderColor: "#465261"
        borderWidth: 2
        shadowColor: "#000000A8"
        shadowBlur: 10
        shadowOffsetY: 5
        transitionMs: motionFast
        onPress: passed => !passed

    Button like at 3,4
        text: "♥︎"
        size: 44
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 68
        maxWidth: 68
        minHeight: 68
        alignX: "center"
        translateY: -45
        focusable: true
        padding: 0
        radius: 34
        backgroundColor: "#FF4D67"
        borderColor: "#FF8A9C"
        borderWidth: 2
        shadowColor: "#000000A8"
        shadowBlur: 12
        shadowOffsetY: 5
        transitionMs: motionFast
        onPress: liked => !liked

    Text profileMeta at 4,1 span columns 4
        text: "98% match  ·  Chestnut  ·  16.1 hands  ·  4 km away"
        color: "#CBD1DA"
        size: 12
        bold: true
        wrap: false

    Text traits at 5,1 span columns 4
        text: "BEACH GALLOPS  ·  SHARES HAY"
        color: "#DCE4E0"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        alignX: "start"
        padding: 8
        backgroundColor: "#141B23"
        borderColor: "#343E4B"
        borderWidth: 1
        radius: 999

    Text bio at 6,1 span columns 4
        text: "Emotionally available. Great listener. Photobomber included."
        color: "#E6E9EE"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 135
        padding: 4

    Text result at 7,1 span columns 4
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#FFE1E5"
        size: 14
        bold: true
        textAlign: "center"
        alignX: "center"
        margin: 18
        padding: 14
        backgroundColor: "#35141AF2"
        borderColor: "#8B3040"
        borderWidth: 1
        radius: 20
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        layoutTransitionMs: motionNormal

    Text passedNote at 7,1 span columns 4
        text: "Passed. The photobomber is taking it personally."
        visible: passed
        color: "#E5E7EB"
        size: 13
        textAlign: "center"
        alignX: "center"
        margin: 18
        padding: 14
        backgroundColor: "#11151BF2"
        borderColor: "#343C47"
        borderWidth: 1
        radius: 20
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#080A0E", surfaceRaisedColor: "#11161D", textColor: "#F7F8FA", textMutedColor: "#9AA1AD", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#303945", shadowColor: "#00000080")
