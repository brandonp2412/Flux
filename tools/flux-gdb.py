import gdb


def _flux_local_name(native_name):
    prefix = "flux__local_"
    if native_name and native_name.startswith(prefix):
        return native_name[len(prefix):]
    return None


def _visible_flux_symbols(frame):
    symbols = []
    seen = set()
    block = frame.block()
    while block is not None:
        for symbol in block:
            name = _flux_local_name(symbol.name)
            if name is None or name in seen:
                continue
            if not (symbol.is_argument or symbol.is_variable):
                continue
            try:
                value = frame.read_var(symbol)
            except gdb.error:
                continue
            symbols.append((name, value))
            seen.add(name)
        block = block.superblock
    return symbols


def _lookup_flux_local(frame, name):
    if not name or not name.replace("_", "a").isalnum() or name[0].isdigit():
        raise gdb.GdbError("expected a Flux local identifier")
    native_name = "flux__local_" + name
    block = frame.block()
    while block is not None:
        for symbol in block:
            if symbol.name != native_name:
                continue
            try:
                return frame.read_var(symbol)
            except gdb.error as error:
                raise gdb.GdbError(str(error))
        block = block.superblock
    raise gdb.GdbError("no visible Flux local named '{}'".format(name))


class FluxLocals(gdb.Command):
    """Show visible Flux locals using source-language names."""

    def __init__(self):
        super().__init__("flux-locals", gdb.COMMAND_DATA)

    def invoke(self, argument, from_tty):
        if argument.strip():
            raise gdb.GdbError("flux-locals takes no arguments")
        frame = gdb.selected_frame()
        symbols = _visible_flux_symbols(frame)
        if not symbols:
            gdb.write("No visible Flux locals.\n")
            return
        for name, value in symbols:
            gdb.write("{} = {}\n".format(name, value))


class FluxPrint(gdb.Command):
    """Print one visible Flux local: flux-print name."""

    def __init__(self):
        super().__init__("flux-print", gdb.COMMAND_DATA)

    def invoke(self, argument, from_tty):
        name = argument.strip()
        value = _lookup_flux_local(gdb.selected_frame(), name)
        gdb.write("{} = {}\n".format(name, value))


_flux_watches = []


def _show_flux_watches(event):
    if not _flux_watches:
        return
    try:
        frame = gdb.selected_frame()
    except gdb.error:
        return
    for name in _flux_watches:
        try:
            value = _lookup_flux_local(frame, name)
            gdb.write("[Flux watch] {} = {}\n".format(name, value))
        except gdb.error:
            gdb.write("[Flux watch] {} = <out of scope>\n".format(name))


class FluxWatch(gdb.Command):
    """Watch one Flux local whenever execution stops: flux-watch name."""

    def __init__(self):
        super().__init__("flux-watch", gdb.COMMAND_DATA)

    def invoke(self, argument, from_tty):
        name = argument.strip()
        value = _lookup_flux_local(gdb.selected_frame(), name)
        if name not in _flux_watches:
            _flux_watches.append(name)
        gdb.write("[Flux watch] {} = {}\n".format(name, value))


class FluxUnwatch(gdb.Command):
    """Remove one Flux local watch: flux-unwatch name."""

    def __init__(self):
        super().__init__("flux-unwatch", gdb.COMMAND_DATA)

    def invoke(self, argument, from_tty):
        name = argument.strip()
        if name not in _flux_watches:
            raise gdb.GdbError("Flux local '{}' is not being watched".format(name))
        _flux_watches.remove(name)
        gdb.write("Stopped watching Flux local '{}'.\n".format(name))


gdb.events.stop.connect(_show_flux_watches)
FluxLocals()
FluxPrint()
FluxWatch()
FluxUnwatch()
