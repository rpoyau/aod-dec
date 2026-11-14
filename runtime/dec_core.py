"""Exact finite reference semantics for the executable Dec subset.

This module declares a finite model, not a derivation of machine physics. A
closure is a retained-start Hamming-1 walk with one explicit structural hinge,
a constant nonzero orientation before it and its opposite afterwards. Full
trace validation precedes certification. Equal event spans are the declared
common ordinal domain used by the coupling fixture. Its count/lag accessors
are not a general RCD constructor, SI clock or complete relational-time model.
"""
from dataclasses import dataclass, asdict
from contextlib import contextmanager
from contextvars import ContextVar
from fractions import Fraction
from hashlib import sha256
import json


@dataclass
class CommitObservation:
    callback: object
    failure_count: int = 0
    last_error: str | None = None


_commit_observation = ContextVar("dec_commit_observation", default=None)


@contextmanager
def observe_commits(callback):
    """Observe successful commitments without changing model return/error state.

    Observer failure is retained separately, not raised as a model error. The
    callback owns persistence and its resource budget; no event queue is kept
    in this runtime. It receives immutable committed values, never an RNG.
    Validation, certificate replay and read-only accessors emit no commitments.
    """
    state = CommitObservation(callback)
    token = _commit_observation.set(state)
    try:
        yield state
    finally:
        _commit_observation.reset(token)


def _committed(kind, value):
    observation = _commit_observation.get()
    if observation is not None and observation.failure_count == 0:
        try:
            observation.callback(kind, value)
        except Exception as exc:
            observation.failure_count += 1
            observation.last_error = type(exc).__name__ + ": " + str(exc)[:2048]

class DecError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise DecError(message)


def integer(value):
    require(type(value) is int, "expected exact int")
    return value


@dataclass(frozen=True)
class T1:
    value: int
    def __post_init__(self):
        require(integer(self.value) in (-1, 0, 1), "t1 outside {-1,0,+1}")


@dataclass(frozen=True)
class T32:
    value: int
    LIMIT = (3 ** 32 - 1) // 2
    def __post_init__(self):
        require(-self.LIMIT <= integer(self.value) <= self.LIMIT, "t32 overflow")

    def bits(self):
        n, result = self.value, 0
        for lane in range(32):
            rem = n % 3
            digit = -1 if rem == 2 else rem
            n = (n - digit) // 3
            result |= {0: 0, 1: 1, -1: 2}[digit] << (2 * lane)
        require(n == 0, "t32 packing overflow")
        return result

    @classmethod
    def from_bits(cls, bits):
        require(0 <= integer(bits) < 2 ** 64, "packed t32 outside u64")
        n = 0
        for lane in range(32):
            code = (bits >> (2 * lane)) & 3
            require(code != 3, "11 is invalid trit encoding; it is not Null")
            n += {0: 0, 1: 1, 2: -1}[code] * 3 ** lane
        return cls(n)


@dataclass(frozen=True)
class Q4:
    value: int
    def __post_init__(self):
        require(0 <= integer(self.value) < 16, "q4 outside 0..15")


@dataclass(frozen=True)
class Event:
    ordinal: int
    kind: str
    source: int
    target: int
    orientation: int


@dataclass(frozen=True)
class Closure:
    begin: int
    end: int
    digest: str


@dataclass(frozen=True)
class Trace:
    scope: str
    lineage: str
    start: Q4
    events: tuple = ()
    closures: tuple = ()
    parents: tuple = ()
    proposal: str = ""


@dataclass(frozen=True)
class Fusion:
    left: Trace
    right: Trace
    scope: str
    lineage: str
    digest: str


@dataclass(frozen=True)
class Coupling:
    left: Trace
    right: Trace
    window: str
    contacts: tuple = ()


TYPE_NAMES = TYPES = frozenset({"int", "bool", "str", "t1", "t32", "q4", "ratio", "trace", "fusion", "coupling"})
_CLASS_NAMES = {int: "int", bool: "bool", str: "str", T1: "t1", T32: "t32", Q4: "q4", Fraction: "ratio", Trace: "trace", Fusion: "fusion", Coupling: "coupling"}


def type_name(value):
    require(type(value) in _CLASS_NAMES, "unsupported value type")
    return _CLASS_NAMES[type(value)]


def make_int(value, kind):
    require(kind in ("int", "t1", "t32", "q4"), "not an integer type")
    return {"int": integer, "t1": T1, "t32": T32, "q4": Q4}[kind](value)


def number(value):
    if type(value) in (T1, T32, Q4):
        return value.value
    require(type(value) in (int, Fraction), "expected numeric value")
    return value


def apply_unary(op, value):
    kind = type_name(value)
    require(kind in ("int", "t1", "t32", "ratio"), "unary arithmetic requires numeric type")
    require(op in ("+", "-"), "unsupported unary operator")
    result = number(value) if op == "+" else -number(value)
    return Fraction(result) if kind == "ratio" else make_int(result, kind)


def apply_binary(op, left, right):
    kind = type_name(left)
    require(kind == type_name(right), "implicit numeric coercion is forbidden")
    if op in ("==", "!="):
        return (left == right) if op == "==" else (left != right)
    require(kind in ("int", "t1", "t32", "ratio"), "arithmetic requires numeric type")
    a, b = number(left), number(right)
    if op in ("<", "<=", ">", ">="):
        return {"<": lambda: a < b, "<=": lambda: a <= b, ">": lambda: a > b, ">=": lambda: a >= b}[op]()
    if op == "/":
        require(b != 0, "zero denominator")
        return Fraction(a) / Fraction(b)
    require(op in ("+", "-", "*", "//", "%"), "unsupported binary operator")
    if op in ("//", "%"):
        require(kind in ("int", "t32"), "integer quotient/remainder required")
        require(b != 0, "zero divisor")
    result = {"+": lambda: a + b, "-": lambda: a - b, "*": lambda: a * b, "//": lambda: a // b, "%": lambda: a % b}[op]()
    return Fraction(result) if kind == "ratio" else make_int(result, kind)


def label(value):
    require(type(value) is str and value.strip() != "", "scope, lineage and window names must be nonempty")
    return value


def digest(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def trace_prefix(t, end):
    return {"scope": t.scope, "lineage": t.lineage, "start": t.start.value,
            "events": [asdict(e) for e in t.events[:end]], "proposal": t.proposal,
            "parents": [p.closures[-1].digest for p in t.parents]}


def _segment(t, begin, end, complete):
    require(0 <= begin <= end <= len(t.events), "invalid trace segment")
    position, orientation, hinged, moves = t.start.value, None, False, 0
    for i, event in enumerate(t.events[begin:end], begin + 1):
        require(type(event) is Event and type(event.ordinal) is int and event.ordinal == i, "invalid or repeated event identity")
        require(type(event.source) is int and event.source == position, "discontinuous retained trace")
        require(type(event.orientation) is int, "event orientation must be an exact integer")
        Q4(event.target)
        if event.kind == "step":
            require(type(event.orientation) is int and event.orientation in (-1, 1), "step requires nonzero orientation")
            require((position ^ event.target).bit_count() == 1, "atomic move must be Hamming-1")
            if orientation is None:
                require(not hinged, "hinge cannot precede outgoing branch")
                orientation = event.orientation
            require(event.orientation == (-orientation if hinged else orientation), "counterbranch orientation requires structural hinge")
            moves += 1
            position = event.target
        elif event.kind == "hinge":
            require(orientation is not None and not hinged, "one structural hinge must follow outgoing branch")
            require(event.target == position and event.orientation == 0, "invalid structural hinge")
            hinged = True
        elif event.kind == "dwell":
            require(event.target == position and event.orientation == 0, "invalid dwell")
        else:
            raise DecError("unknown retained event kind")
    if complete:
        require(moves >= 2 and hinged, "closure needs outgoing branch, hinge and counterbranch")
        require(position == t.start.value, "open trace does not return to retained start")
        require(t.events[end - 1].kind == "step", "closure must identify a return event, not dwell")
        require(t.events[end - 1].orientation == -orientation, "return event is not counterbranch")
    return position


def validate_trace(t, depth=0):
    require(type(t) is Trace and depth < 64, "invalid or excessively nested trace")
    label(t.scope); label(t.lineage)
    require(type(t.start) is Q4, "trace retained start must be q4")
    Q4(t.start.value)
    require(type(t.events) is tuple and type(t.closures) is tuple and type(t.parents) is tuple, "trace records must be immutable")
    if t.parents:
        require(len(t.parents) == 2, "fusion requires two retained parents in this subset")
        for p in t.parents:
            validate_trace(p, depth + 1)
            require(p.closures and p.closures[-1].end == len(p.events), "fusion parent is not closed")
        require((t.parents[0].scope,t.parents[0].lineage) != (t.parents[1].scope,t.parents[1].lineage), "fusion parents must have distinct lineages")
        require(t.scope not in (t.parents[0].scope,t.parents[1].scope), "outer scope must differ from inner scopes")
        expected = fusion_digest(t.parents[0], t.parents[1], t.scope, t.lineage)
        require(t.proposal == expected, "outer trace does not match retained fusion proposal")
    else:
        require(t.proposal == "", "orphan fusion proposal")
    begin = 0
    for certificate in t.closures:
        require(type(certificate) is Closure and type(certificate.begin) is int and type(certificate.end) is int and certificate.begin == begin and begin < certificate.end <= len(t.events), "invalid/replayed closure range")
        _segment(t, begin, certificate.end, True)
        require(certificate.digest == digest(trace_prefix(t, certificate.end)), "closure certificate does not match retained history")
        begin = certificate.end
    return _segment(t, begin, len(t.events), False)


def new_trace(scope, lineage, start):
    t = Trace(label(scope), label(lineage), start)
    validate_trace(t)
    _committed("core.start", t)
    return t


def append_event(t, kind, target=None, orientation=0):
    require(target is None or type(target) is Q4, "step target must be q4")
    position = validate_trace(t)
    destination = position if target is None else target.value
    event = Event(len(t.events) + 1, kind, position, destination, orientation)
    out = Trace(t.scope, t.lineage, t.start, t.events + (event,), t.closures, t.parents, t.proposal)
    validate_trace(out)
    _committed("core.transition", out)
    return out


def close_trace(t):
    validate_trace(t)
    begin = t.closures[-1].end if t.closures else 0
    if begin == len(t.events) and t.closures:
        return t  # Persistent certification/replay does not create a return event.
    _segment(t, begin, len(t.events), True)
    certificate = Closure(begin, len(t.events), digest(trace_prefix(t, len(t.events))))
    out = Trace(t.scope, t.lineage, t.start, t.events, t.closures + (certificate,), t.parents, t.proposal)
    validate_trace(out)
    _committed("core.closure", out)
    return out


def phases(t):
    validate_trace(t)
    return len(t.closures)


def beats(t):
    return max(0, phases(t) - 1)


def rate(t):
    validate_trace(t)
    require(t.events, "rate requires a positive committed event span")
    return Fraction(phases(t), len(t.events))


def fusion_digest(a, b, scope, lineage):
    return digest({"parents": [a.closures[-1].digest,b.closures[-1].digest], "scope": scope, "lineage": lineage})


def fold(a, b, scope, lineage):
    for t in (a,b):
        validate_trace(t)
        require(t.closures and t.closures[-1].end == len(t.events), "fold requires closed retained inner inputs")
    require((a.scope,a.lineage) != (b.scope,b.lineage), "fold requires distinct inner lineages")
    label(scope); label(lineage)
    require(scope not in (a.scope,b.scope), "outer scope must differ from inner scopes")
    return Fusion(a,b,scope,lineage,fusion_digest(a,b,scope,lineage))


def validate_fusion(f):
    require(type(f) is Fusion, "expected fusion proposal")
    expected = fold(f.left,f.right,f.scope,f.lineage)
    require(expected == f, "modified fusion proposal")


def outer(f, start):
    validate_fusion(f)
    t = Trace(f.scope,f.lineage,start,parents=(f.left,f.right),proposal=f.digest)
    validate_trace(t)
    _committed("core.start", t)
    return t


def fuse(f, t):
    validate_fusion(f); validate_trace(t)
    require((t.scope,t.lineage,t.parents,t.proposal) == (f.scope,f.lineage,(f.left,f.right),f.digest), "outer trace belongs to a different proposal")
    require(t.closures and t.closures[-1].end == len(t.events), "fusion requires independently witnessed outer closure")
    _committed("core.fusion", t)
    return t


def coupling(a, b, window):
    validate_trace(a); validate_trace(b); label(window)
    require(a.scope == b.scope and a.lineage != b.lineage, "coupling needs distinct lineages in common scope")
    require(len(a.events) == len(b.events) and len(a.events) > 0, "coupling needs equal positive committed event spans")
    return Coupling(a,b,window)


def validate_coupling(c):
    require(type(c) is Coupling and type(c.contacts) is tuple, "invalid coupling")
    coupling(c.left,c.right,c.window)
    a_ids = {x.end for x in c.left.closures}
    b_ids = {x.end for x in c.right.closures}
    prev_a = prev_b = 0
    for pair in c.contacts:
        require(type(pair) is tuple and len(pair) == 2, "contact pairs must be immutable return-identity tuples")
        a,b = pair
        require(type(a) is int and type(b) is int and a in a_ids and b in b_ids, "contact must bind certified return events")
        require(a > prev_a and b > prev_b, "contacts must be distinct, ordered and unreplayed")
        prev_a,prev_b = a,b


def contact(c, a, b):
    validate_coupling(c)
    out = Coupling(c.left,c.right,c.window,c.contacts+((a,b),))
    validate_coupling(out)
    _committed("core.contact", out)
    return out


def _comparison(c):
    validate_coupling(c)
    require(len(c.contacts) >= 2 and phases(c.left) >= 2 and phases(c.right) >= 2, "comparison needs recurrent lineages and repeated bound contacts")


def cadence(c):
    _comparison(c)
    return Fraction(phases(c.left),phases(c.right))


def lag(c):
    _comparison(c)
    return c.contacts[-1][0] - c.contacts[-1][1]


def drift(c):
    _comparison(c)
    return lag(c) - (c.contacts[0][0] - c.contacts[0][1])


def normalize_weights(weights):
    require(type(weights) in (tuple,list) and weights, "weights must be a nonempty sequence")
    for w in weights:
        require(type(w) in (int,Fraction) and w >= 0, "weights must be exact and nonnegative")
    total = sum(weights)
    require(total > 0, "normalization requires positive total weight")
    return tuple(Fraction(w,total) for w in weights)


def finite_afc(memory, kernel, region):
    """Normalized finite AFC fixture; no physical pressure or geometry inferred."""
    require(type(memory) in (tuple,list) and memory, "nonempty finite memory required")
    p = tuple(Fraction(v) for v in memory)
    require(all(type(v) in (int,Fraction) for v in memory) and all(v>=0 for v in p) and sum(p)==1, "invalid normalized memory")
    n=len(p)
    require(len(kernel)==n and all(len(row)==n for row in kernel), "kernel shape mismatch")
    P=tuple(tuple(Fraction(v) for v in row) for row in kernel)
    require(all(type(v) in (int,Fraction) for row in kernel for v in row), "kernel must be exact")
    require(all(all(v>=0 for v in row) and sum(row)==1 for row in P), "kernel rows must be nonnegative and normalized")
    require(type(region) in (tuple,list,set,frozenset), "region must be an explicit finite collection")
    require(all(type(i) is int and 0<=i<n for i in region), "region outside finite domain")
    R=set(region)
    phi=tuple(tuple(p[i]*P[i][j]-p[j]*P[j][i] for j in range(n)) for i in range(n))
    div=tuple(sum(row) for row in phi)
    boundary=sum(phi[i][j] for i in R for j in range(n) if j not in R)
    require(sum(div[i] for i in R)==boundary, "finite Stokes mismatch")
    return {"flux":phi,"divergence":div,"boundary":boundary,"next_memory":tuple(sum(p[i]*P[i][j] for i in range(n)) for j in range(n))}


def probability(negative, hinge_weight, positive, role):
    weights = normalize_weights((negative, hinge_weight, positive))
    return weights[role.value + 1]


def _to_int(value):
    n=number(value)
    require(not isinstance(n,Fraction) or n.denominator==1, "ratio-to-int would lose a remainder")
    return int(n)


def _ratio(a,b):
    require(b != 0,"zero denominator")
    return Fraction(a,b)


BUILTIN_SIGNATURES = {
    "t1":[(("int",),"t1")],"t32":[(("int",),"t32")],"q4":[(("int",),"q4")],
    "int":[((k,),"int") for k in ("int","t1","t32","q4","ratio")],
    "ratio":[(("int","int"),"ratio"),(("int",),"ratio")],
    "pack":[(("t32",),"int")],"unpack":[(("int",),"t32")],
    "trace":[(("str","str","q4"),"trace")],
    "step":[(("trace","q4","t1"),"trace")],
    "hinge":[(("trace",),"trace")],"dwell":[(("trace",),"trace")],"close":[(("trace",),"trace")],
    "phases":[(("trace",),"int")],"beats":[(("trace",),"int")],"rate":[(("trace",),"ratio")],
    "fold":[(("trace","trace","str","str"),"fusion")],"outer":[(("fusion","q4"),"trace")],"fuse":[(("fusion","trace"),"trace")],
    "coupling":[(("trace","trace","str"),"coupling")],"contact":[(("coupling","int","int"),"coupling")],
    "cadence":[(("coupling",),"ratio")],"lag":[(("coupling",),"int")],"drift":[(("coupling",),"int")],
    "probability":[(("int","int","int","t1"),"ratio")],
    "neg":[((k,),k) for k in ("int","t1","t32","ratio")],
}


def builtin_signatures():
    return BUILTIN_SIGNATURES


def call_builtin(name,args):
    require(name in BUILTIN_SIGNATURES,"unknown builtin: "+name)
    sig=tuple(type_name(v) for v in args)
    require(any(sig==s[0] for s in BUILTIN_SIGNATURES[name]),"unlisted signature for "+name+str(sig))
    functions={"t1":T1,"t32":T32,"q4":Q4,"int":_to_int,"ratio":lambda *a: _ratio(a[0],a[1] if len(a)>1 else 1),
               "pack":lambda a:a.bits(),"unpack":T32.from_bits,"trace":new_trace,
               "step":lambda t,q,s:append_event(t,"step",q,s.value),"hinge":lambda t:append_event(t,"hinge"),"dwell":lambda t:append_event(t,"dwell"),
               "close":close_trace,"phases":phases,"beats":beats,"rate":rate,"fold":fold,"outer":outer,"fuse":fuse,
               "coupling":coupling,"contact":contact,"cadence":cadence,"lag":lag,"drift":drift,"neg":lambda a:apply_unary("-",a),"probability":probability}
    return functions[name](*args)


def render(value):
    kind=type_name(value)
    if kind in ("t1","t32","q4"):
        return f"{kind}({value.value})"
    if kind=="ratio":
        return f"{value.numerator}/{value.denominator}"
    if kind=="bool":return "true" if value else "false"
    if kind in ("int","str"):return str(value)
    return json.dumps(asdict(value),sort_keys=True,separators=(",", ":"))
