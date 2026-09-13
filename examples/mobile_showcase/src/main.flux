view HorseTinder {
    state decision: str = ""
    derived actionOffset: i64 = (windowWidth - 354) / 2
    grid columns: 1fr 80 40 80 1fr
    grid rows: auto 1fr 60 36 auto 80
    grid gap: 12
    grid padding: 20
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "HORSE TINDER"
        color: "#F6F0E5"
        size: 16
        bold: true
        wrap: false
        letterSpacing: 2
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "4 KM  •  AVAILABLE"
        color: "#B9B7A9"
        size: 9
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#17241F"
        borderColor: "#3A4A41"
        borderWidth: 1
        radius: radiusPill

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 395
        maxHeight: 475
        backgroundColor: "#141E1A"
        borderColor: "#46564C"
        borderWidth: 1
        radius: 26
        shadowColor: "#00000099"
        shadowBlur: 14
        shadowOffsetY: 7

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "#FAF5EA"
        size: 35
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98% MATCH"
        color: "#172018"
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
        backgroundColor: "#D0B27C"
        borderColor: "#E9D4AA"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 3
        text: "Chestnut mare  •  16.1 hands  •  Nearby"
        color: "#BBB9AC"
        size: 12
        bold: true
        wrap: false
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Fond of beach gallops, excellent hay manners, and emotionally available company."
        color: "#EEE8DD"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 145
        padding: 16
        backgroundColor: "#15211CF2"
        borderColor: "#3D4D44"
        borderWidth: 1
        radius: 18

    Image pass at 6,2
        source: "assets/icon-pass.png"
        alt: "Pass"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 78
        minHeight: 78
        padding: 20
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        clip: true
        backgroundColor: "#18231F"
        borderColor: "#59675F"
        borderWidth: 1
        radius: 39
        shadowColor: "#00000080"
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
        minWidth: 78
        minHeight: 78
        padding: 19
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        clip: true
        backgroundColor: "#B48A52"
        borderColor: "#D4B17E"
        borderWidth: 1
        radius: 39
        shadowColor: "#B48A524D"
        shadowBlur: 12
        shadowOffsetY: 5
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "liked"

    Text result at 6,1 span columns 5
        text: "A MATCH  •  Buttercup would like to meet"
        visible: decision == "liked"
        color: "#F7EBD7"
        size: 14
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#49371FF2"
        borderColor: "#967546"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "Passed  •  Another profile is waiting"
        visible: decision == "passed"
        color: "#D8D5CC"
        size: 13
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#17211DF2"
        borderColor: "#455249"
        borderWidth: 1
        radius: 18
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#0D1511", surfaceRaisedColor: "#15211C", textColor: "#F6F0E5", textMutedColor: "#BBB9AC", accentColor: "#B48A52", onAccentColor: "#172018", outlineColor: "#3D4D44", shadowColor: "#00000099")
