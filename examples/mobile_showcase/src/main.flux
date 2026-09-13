view HorseTinder {
    state decision: str = ""
    grid columns: 1fr 80 40 80 1fr
    grid rows: 50 1fr 60 36 auto 80
    grid gap: 12
    grid padding: 20
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "HORSE TINDER"
        color: "#2E2528"
        size: 17
        bold: true
        wrap: false
        letterSpacing: 2
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "4 KM AWAY"
        color: "#7F706F"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#F4EDE8"
        borderColor: "#D8C9C1"
        borderWidth: 1
        radius: radiusPill

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 390
        maxHeight: 470
        backgroundColor: "#EEE4DE"
        borderColor: "#D8CBC4"
        borderWidth: 1
        radius: 28
        shadowColor: "#5E433326"
        shadowBlur: 12
        shadowOffsetY: 6

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "#2D2526"
        size: 35
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98% MATCH"
        color: "#FFFFFF"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 9
        minWidth: 102
        minHeight: 36
        maxHeight: 36
        backgroundColor: "#9E4058"
        borderColor: "#C97A8F"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 3
        text: "Chestnut mare  •  16.1 hands  •  Online now"
        color: "#75696A"
        size: 12
        bold: true
        wrap: false
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Beach gallops, generous with hay, and emotionally available. Photobomber included."
        color: "#403638"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 145
        padding: 16
        backgroundColor: "#FFFDFC"
        borderColor: "#DCCFC8"
        borderWidth: 1
        radius: 18
        shadowColor: "#6048391A"
        shadowBlur: 6
        shadowOffsetY: 3

    Image pass at 6,2
        source: "assets/icon-pass.png"
        alt: "Pass"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 76
        minHeight: 76
        padding: 20
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#453B3C"
        borderColor: "#6A5C5B"
        borderWidth: 1
        radius: 38
        shadowColor: "#60483926"
        shadowBlur: 8
        shadowOffsetY: 4
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "passed"

    Image like at 6,4
        source: "assets/icon-heart.png"
        alt: "Like"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Like Buttercup"
        minWidth: 76
        minHeight: 76
        padding: 19
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#9E4058"
        borderColor: "#C8788E"
        borderWidth: 1
        radius: 38
        shadowColor: "#9E40584D"
        shadowBlur: 12
        shadowOffsetY: 5
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "liked"

    Text result at 6,1 span columns 5
        text: "A lovely match  •  Buttercup likes you too"
        visible: decision == "liked"
        color: "#6F283D"
        size: 14
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#F9E8ED"
        borderColor: "#D5A4B1"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "Passed  •  The photobomber remains unconvinced"
        visible: decision == "passed"
        color: "#5F5554"
        size: 13
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#F7F1ED"
        borderColor: "#D8CBC4"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "light", surfaceColor: "#F8F2EE", surfaceRaisedColor: "#FFFDFC", textColor: "#2D2526", textMutedColor: "#75696A", accentColor: "#9E4058", onAccentColor: "#FFFFFF", outlineColor: "#D8CBC4", shadowColor: "#60483926")
