view Today {
    state hydrated: bool = true
    state mindful: bool = false
    grid columns: 1fr 1fr
    grid rows: auto auto auto auto auto auto auto auto
    grid gap: 12
    grid padding: 18
    grid scroll: true

    Text eyebrow at 1,1 span columns 2
        text: "THURSDAY · 10 SEPTEMBER"
        color: "#64748B"
        size: 12
        bold: true
        letterSpacing: 1

    Text title at 2,1 span columns 2
        text: "Good evening"
        color: "#0F172A"
        size: 32
        bold: true
        marginBottom: 4

    Text hero at 3,1 span columns 2
        text: "A calmer day starts with one small choice."
        color: "#0F172A"
        size: 22
        bold: true
        wrap: true
        maxLines: 3
        padding: 22
        minHeight: 118
        backgroundColor: "#EDE9FE"
        radius: 22
        shadowColor: "#64748B30"
        shadowBlur: 18
        shadowOffsetY: 6

    Text sleep at 4,1
        text: "SLEEP\n7h 42m\n96% of goal"
        color: "#312E81"
        size: 18
        bold: true
        wrap: true
        padding: 18
        minHeight: 100
        backgroundColor: "#EEF2FF"
        radius: 18

    Text water at 4,2
        text: "WATER\n1.8 L\n90% of goal"
        color: "#075985"
        size: 18
        bold: true
        wrap: true
        padding: 18
        minHeight: 100
        backgroundColor: "#E0F2FE"
        radius: 18

    Text streak at 5,1 span columns 2
        text: "12 day streak   ·   Best 28 days"
        color: "#14532D"
        size: 17
        bold: true
        padding: 18
        backgroundColor: "#DCFCE7"
        radius: 18

    Text mood at 6,1 span columns 2
        text: "Mood & energy\nMon  ·  Tue  ·  Wed  ·  Thu  ·  Fri  ·  Sat  ·  Sun\n  62      70       84       73       68       82       91"
        color: "#334155"
        size: 15
        wrap: true
        padding: 18
        minHeight: 116
        backgroundColor: "#FFFFFF"
        borderColor: "#E2E8F0"
        borderWidth: 1
        radius: 18
        shadowColor: "#64748B24"
        shadowBlur: 14
        shadowOffsetY: 4

    Toggle waterGoal at 7,1 span columns 2
        label: "Drink 2 L of water"
        checked: hydrated
        padding: 16
        backgroundColor: "#FFFFFF"
        borderColor: "#E2E8F0"
        borderWidth: 1
        radius: 16
        onChange: hydrated => !hydrated

    Button breathe at 8,1 span columns 2
        text: "Take a mindful minute"
        primary: true
        padding: 14
        radius: 16
        onPress: mindful => !mindful
}

app Today(title: "Flux Showcase")
