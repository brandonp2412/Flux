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


_I64_MIN = -(1 << 63)
_I64_MAX = (1 << 63) - 1


def _tokenize_flux_expression(source):
    tokens = []
    index = 0
    while index < len(source):
        char = source[index]
        if char.isspace():
            index += 1
            continue
        pair = source[index:index + 2]
        if pair in ("&&", "||", "==", "!=", "<=", ">="):
            tokens.append(("operator", pair))
            index += 2
            continue
        if char in "()+-*/!<>":
            tokens.append(("operator", char))
            index += 1
            continue
        if char.isdigit():
            start = index
            while index < len(source) and source[index].isdigit():
                index += 1
            tokens.append(("i64", source[start:index]))
            continue
        if char.isalpha() or char == "_":
            start = index
            index += 1
            while index < len(source) and (source[index].isalnum() or source[index] == "_"):
                index += 1
            value = source[start:index]
            if value in ("true", "false"):
                tokens.append(("bool", value))
            else:
                tokens.append(("identifier", value))
            continue
        if char == '"':
            index += 1
            value = []
            while index < len(source):
                char = source[index]
                if char == '"':
                    index += 1
                    tokens.append(("str", "".join(value)))
                    break
                if char == "\\":
                    index += 1
                    if index >= len(source):
                        raise gdb.GdbError("unterminated escape in Flux string literal")
                    escaped = source[index]
                    escapes = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"'}
                    if escaped not in escapes:
                        raise gdb.GdbError("unsupported Flux string escape '\\{}'".format(escaped))
                    value.append(escapes[escaped])
                    index += 1
                    continue
                value.append(char)
                index += 1
            else:
                raise gdb.GdbError("unterminated Flux string literal")
            continue
        raise gdb.GdbError("unexpected character '{}' in Flux expression".format(char))
    tokens.append(("end", ""))
    return tokens


class _FluxExpressionParser:
    def __init__(self, source):
        self.tokens = _tokenize_flux_expression(source)
        self.index = 0

    def _peek(self, value=None):
        token = self.tokens[self.index]
        if value is None:
            return token
        return token[1] == value

    def _take(self):
        token = self.tokens[self.index]
        self.index += 1
        return token

    def parse(self):
        expression = self._parse_or()
        token = self._peek()
        if token[0] != "end":
            raise gdb.GdbError("unexpected token '{}' in Flux expression".format(token[1]))
        return expression

    def _parse_or(self):
        expression = self._parse_and()
        while self._peek("||"):
            self._take()
            expression = ("binary", "||", expression, self._parse_and())
        return expression

    def _parse_and(self):
        expression = self._parse_equality()
        while self._peek("&&"):
            self._take()
            expression = ("binary", "&&", expression, self._parse_equality())
        return expression

    def _parse_equality(self):
        expression = self._parse_comparison()
        while self._peek() in (("operator", "=="), ("operator", "!=")):
            operator = self._take()[1]
            expression = ("binary", operator, expression, self._parse_comparison())
        return expression

    def _parse_comparison(self):
        expression = self._parse_additive()
        while self._peek() in (
            ("operator", "<"),
            ("operator", "<="),
            ("operator", ">"),
            ("operator", ">="),
        ):
            operator = self._take()[1]
            expression = ("binary", operator, expression, self._parse_additive())
        return expression

    def _parse_additive(self):
        expression = self._parse_multiplicative()
        while self._peek() in (("operator", "+"), ("operator", "-")):
            operator = self._take()[1]
            expression = ("binary", operator, expression, self._parse_multiplicative())
        return expression

    def _parse_multiplicative(self):
        expression = self._parse_unary()
        while self._peek() in (("operator", "*"), ("operator", "/")):
            operator = self._take()[1]
            expression = ("binary", operator, expression, self._parse_unary())
        return expression

    def _parse_unary(self):
        if self._peek("!") or self._peek("-"):
            operator = self._take()[1]
            return ("unary", operator, self._parse_unary())
        return self._parse_primary()

    def _parse_primary(self):
        token = self._take()
        if token[0] == "i64":
            value = int(token[1])
            if value > _I64_MAX:
                raise gdb.GdbError("Flux i64 literal is out of range")
            return ("literal", "i64", value)
        if token[0] == "bool":
            return ("literal", "bool", token[1] == "true")
        if token[0] == "str":
            return ("literal", "str", token[1])
        if token[0] == "identifier":
            return ("local", token[1])
        if token == ("operator", "("):
            expression = self._parse_or()
            if not self._peek(")"):
                raise gdb.GdbError("expected ')' in Flux expression")
            self._take()
            return expression
        if token[0] == "end":
            raise gdb.GdbError("expected a Flux expression")
        raise gdb.GdbError("expected a Flux value, got '{}'".format(token[1]))


def _flux_value_from_gdb(value):
    value_type = value.type.strip_typedefs()
    if value_type.code == gdb.TYPE_CODE_BOOL:
        return ("bool", bool(value))
    if value_type.code in (gdb.TYPE_CODE_INT, gdb.TYPE_CODE_ENUM):
        return ("i64", int(value))
    if value_type.code == gdb.TYPE_CODE_PTR:
        target = value_type.target().strip_typedefs()
        if target.code == gdb.TYPE_CODE_INT and target.sizeof == 1:
            if int(value) == 0:
                raise gdb.GdbError("null string/error local cannot be evaluated as a Flux str")
            try:
                return ("str", value.string())
            except gdb.error as error:
                raise gdb.GdbError(str(error))
    raise gdb.GdbError("this Flux local type is not supported by flux-eval yet")


def _require_kind(value, kind, operator):
    if value[0] != kind:
        raise gdb.GdbError("operator '{}' requires {} operands".format(operator, kind))
    return value[1]


def _checked_i64(value, operation):
    if value < _I64_MIN or value > _I64_MAX:
        raise gdb.GdbError("Flux integer {} overflow".format(operation))
    return ("i64", value)


def _evaluate_flux_expression(node, frame):
    kind = node[0]
    if kind == "literal":
        return (node[1], node[2])
    if kind == "local":
        return _flux_value_from_gdb(_lookup_flux_local(frame, node[1]))
    if kind == "unary":
        operator = node[1]
        value = _evaluate_flux_expression(node[2], frame)
        if operator == "!":
            return ("bool", not _require_kind(value, "bool", operator))
        number = _require_kind(value, "i64", operator)
        if number == _I64_MIN:
            raise gdb.GdbError("Flux integer negation overflow")
        return ("i64", -number)

    operator = node[1]
    left = _evaluate_flux_expression(node[2], frame)
    if operator == "&&":
        left_value = _require_kind(left, "bool", operator)
        if not left_value:
            return ("bool", False)
        right = _evaluate_flux_expression(node[3], frame)
        return ("bool", _require_kind(right, "bool", operator))
    if operator == "||":
        left_value = _require_kind(left, "bool", operator)
        if left_value:
            return ("bool", True)
        right = _evaluate_flux_expression(node[3], frame)
        return ("bool", _require_kind(right, "bool", operator))

    right = _evaluate_flux_expression(node[3], frame)
    if operator in ("==", "!="):
        if left[0] != right[0]:
            raise gdb.GdbError("operator '{}' requires operands of the same Flux type".format(operator))
        equal = left[1] == right[1]
        return ("bool", equal if operator == "==" else not equal)
    if operator in ("<", "<=", ">", ">="):
        left_value = _require_kind(left, "i64", operator)
        right_value = _require_kind(right, "i64", operator)
        if operator == "<":
            return ("bool", left_value < right_value)
        if operator == "<=":
            return ("bool", left_value <= right_value)
        if operator == ">":
            return ("bool", left_value > right_value)
        return ("bool", left_value >= right_value)

    left_value = _require_kind(left, "i64", operator)
    right_value = _require_kind(right, "i64", operator)
    if operator == "+":
        return _checked_i64(left_value + right_value, "addition")
    if operator == "-":
        return _checked_i64(left_value - right_value, "subtraction")
    if operator == "*":
        return _checked_i64(left_value * right_value, "multiplication")
    if right_value == 0:
        raise gdb.GdbError("Flux integer division by zero")
    if left_value == _I64_MIN and right_value == -1:
        raise gdb.GdbError("Flux integer division overflow")
    magnitude = abs(left_value) // abs(right_value)
    quotient = -magnitude if (left_value < 0) != (right_value < 0) else magnitude
    return ("i64", quotient)


def _format_flux_value(value):
    if value[0] == "bool":
        return "true" if value[1] else "false"
    if value[0] == "str":
        escaped = value[1].replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
        return '"{}"'.format(escaped)
    return str(value[1])


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


class FluxEval(gdb.Command):
    """Evaluate a side-effect-free Flux expression over visible primitive locals."""

    def __init__(self):
        super().__init__("flux-eval", gdb.COMMAND_DATA)

    def invoke(self, argument, from_tty):
        source = argument.strip()
        if not source:
            raise gdb.GdbError("flux-eval requires a Flux expression")
        expression = _FluxExpressionParser(source).parse()
        value = _evaluate_flux_expression(expression, gdb.selected_frame())
        gdb.write("{} = {}\n".format(source, _format_flux_value(value)))


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
FluxEval()
FluxPrint()
FluxWatch()
FluxUnwatch()
