"""Finite retained-history continuation model, supplied to the unchanged engine.

Unit formation/relation opportunities support actual C6 return recipes. The
integer inventory determines probabilities; no flip-rate coefficient is used.
The opportunity routing, contact/window conventions and finite horizon are
explicit application choices, not an intrinsically selected physical law.
"""
from dataclasses import dataclass, fields, is_dataclass, replace
from fractions import Fraction
from itertools import permutations
from math import lcm

from .vendor import aod_manual_native as native

HORIZON = 6
WINDOW = 2
CLIP = 6
SCOPE = "retained-continuation:B0"
WORDS = tuple(permutations(range(4), 3))
TICKET_DENOMINATOR = 36 * lcm(*range(1, 87))
STATE_FIELDS = ("tick", "incoming_a", "incoming_b", "chi_a", "chi_b",
                "count_a", "count_b", "since_a", "since_b", "contact_age",
                "previous_ref_a", "previous_ref_b", "r0a", "p0a", "r1a", "p1a",
                "r0b", "p0b", "r1b", "p1b")
STATE_MIN = (0,0,0,-1,-1,3,2,0,0,0,0,0,-42,0,-42,0,-42,0,-42,0)
STATE_MAX = (6,3,3,1,1,9,8,6,6,3,6,6,42,1,42,1,42,1,42,1)
STATE_RADICES = tuple(b-a+1 for a,b in zip(STATE_MIN, STATE_MAX))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_state(state):
    require(type(state) is tuple and len(state) == len(STATE_FIELDS), "20-coordinate tuple required")
    require(all(type(v) is int and lo <= v <= hi
                for v,lo,hi in zip(state, STATE_MIN, STATE_MAX)), "state outside exact finite domain")
    require(state[3] in (-1,1) and state[4] in (-1,1), "handedness must be -1 or +1")
    for value, present in ((12,13),(14,15),(16,17),(18,19)):
        require(state[present] or state[value] == 0, "absent relation cannot carry a value")
    return state


def initial_state():
    return (0,2,0,1,-1,3,2,0,0,0,3,2,0,0,0,0,0,0,0,0)


def encode_state(state):
    validate_state(state)
    value = 0
    for x,lo,radix in zip(state, STATE_MIN, STATE_RADICES):
        value = value * radix + x - lo
    return value


def decode_state(value):
    require(type(value) is int and value >= 0, "nonnegative integer state code required")
    result = []
    for lo,radix in reversed(tuple(zip(STATE_MIN, STATE_RADICES))):
        value, digit = divmod(value, radix)
        result.append(digit + lo)
    require(value == 0, "state code overflow")
    return validate_state(tuple(reversed(result)))


def word_vertices(word):
    require(type(word) is int and 0 <= word < len(WORDS), "unknown C6 word")
    a,b,c = (1 << axis for axis in WORDS[word])
    return (0,a,a^b,a^b^c,b^c,c,0)


def eligible_words(incoming, mask):
    require(type(incoming) is int and 0 <= incoming < 4, "incoming slot outside Q4")
    require(type(mask) is int and 0 <= mask < 16, "exact four-axis mask required")
    return tuple(i for i,axes in enumerate(WORDS)
                 if axes[0] != incoming and all(mask & (1 << a) for a in axes))


def _counts(state, carrier):
    minus = plus = 1
    offset = 12 + 4 * carrier
    for i in (offset, offset + 2):
        if state[i+1]:
            minus += max(0, -state[i])
            plus += max(0, state[i])
    return minus, plus


def _select(words, counts, ticket):
    require(type(ticket) is int and 0 <= ticket < TICKET_DENOMINATOR, "ticket outside exact common grid")
    total = len(words) * sum(counts)
    if not total:
        return -1, 0, -1, 0
    require(TICKET_DENOMINATOR % total == 0, "declared grid does not represent inventory")
    slot = ticket * total // TICKET_DENOMINATOR
    for hand, multiplicity in zip((-1,1), counts):
        for word in words:
            if slot < multiplicity:
                return word, hand, slot, total
            slot -= multiplicity
    raise ArithmeticError("counted selection escaped inventory")


def transition(state, mask_a, mask_b, ticket_a, ticket_b):
    """Pure compact update; counts are tokens PER WORD, before multiplying by G."""
    validate_state(state)
    require(state[0] < HORIZON, "finite horizon exhausted")
    words = (eligible_words(state[1],mask_a), eligible_words(state[2],mask_b))
    counts = (_counts(state,0),_counts(state,1))
    choices = tuple(_select(w,c,t) for w,c,t in zip(words,counts,(ticket_a,ticket_b)))
    completed = tuple(c[0] >= 0 for c in choices)
    out = list(state)
    for i,(word,hand,_,_) in enumerate(choices):
        if completed[i]:
            out[1+i] = WORDS[word][-1]
            out[3+i] = hand
            out[5+i] += 1
            out[7+i] += 1
    contact = all(completed)
    new = ((0,0),(0,0))
    if contact:
        if state[9] + 1 <= WINDOW:
            delta = out[5] - out[6]
            da = min(CLIP,3+state[10]+out[7])
            db = min(CLIP,3+state[11]+out[8])
            new = ((-out[4]*db*delta,1),(out[3]*da*delta,1))
        out[9] = 0
        out[10],out[11] = out[7],out[8]
        out[7] = out[8] = 0
    else:
        out[9] = min(3,state[9]+1)
    for i,(value,presence) in enumerate(new):
        offset = 12+4*i
        out[offset+2:offset+4] = state[offset:offset+2]
        out[offset:offset+2] = [value,presence]
    out[0] += 1
    next_state = validate_state(tuple(out))
    return {"next_state":next_state, "counts":counts,
            "geometry_counts":tuple(map(len,words)), "totals":tuple(c[3] for c in choices),
            "words":tuple(c[0] for c in choices), "hands":tuple(c[1] for c in choices),
            "token_ranks":tuple(c[2] for c in choices), "completed":completed,
            "contact":contact, "new_relations":new}


def atomic_prefix_counts(state, carrier, mask, prefix=(0,), hand=None):
    """Push the complete inventory onto its next H1 edge; hinge is separate.

    Prefix is a tuple of already executed vertices, beginning at retained zero.
    When a hand is supplied it conditions on that handed recipe as well.
    """
    validate_state(state)
    require(type(carrier) is int and carrier in (0,1), "carrier index required")
    require(type(prefix) is tuple and 1 <= len(prefix) <= 7 and prefix[0] == 0
            and all(type(v) is int and 0 <= v < 16 for v in prefix), "exact Q4 vertex prefix required")
    require(hand is None or (type(hand) is int and hand in (-1,1)), "invalid hand conditioning")
    counts = _counts(state,carrier)
    rows = {}
    for word in eligible_words(state[1+carrier],mask):
        vertices = word_vertices(word)
        if vertices[:len(prefix)] != prefix or len(prefix) == 7:
            continue
        edge = (vertices[len(prefix)-1], vertices[len(prefix)])
        multiplicity = sum(counts) if hand is None else counts[0 if hand == -1 else 1]
        rows[edge] = rows.get(edge,0) + multiplicity
    return rows


@dataclass(frozen=True)
class Opportunity:
    identity: str
    hand: int
    source_kind: str
    source_id: str
    unit_id: str
    return_id: str


@dataclass(frozen=True)
class ContactPair:
    identity: str
    tick: int
    committed: int
    contacts: tuple
    reflection_occurrences: tuple
    prefix_counts: tuple


@dataclass(frozen=True)
class RetainedRelation:
    identity: str
    target: int
    created_tick: int
    input_cut: int
    retained_at: int
    contact_ids: tuple
    rcd: object
    support_units: tuple
    unmatched_returns: tuple
    value: int
    tokens: tuple


@dataclass(frozen=True)
class Attempt:
    masks: tuple
    tickets: tuple
    row: dict
    selected_opportunities: tuple


@dataclass(frozen=True)
class World:
    state: tuple
    waves: tuple
    components: tuple
    contacts: tuple
    relations: tuple
    attempts: tuple
    fields: tuple
    disabled_relations: tuple = ()
    interventions: tuple = ()


def _registry(pairs):
    return {c.certificate_digest:c for _,c in pairs}


def _target(lineage, occurrence, start, word, hand, components):
    vertices = word_vertices(word)
    require(len(set(vertices[:-1])) == 6 and vertices[-1] == vertices[0], "C6 realization required")
    trace = native.trace(lineage,occurrence,vertices=vertices,turn=3,start_index=start,
                         scope=SCOPE,chirality=hand)
    certificate = native.certify(trace)
    pairs = components + ((trace,certificate),)
    registry = _registry(pairs)
    ids = tuple(c.return_id for _,c in components)
    motif = native.motif_witness("1:2",trace,((ids,),ids),components,registry,
                                 enclosure=6,chirality=hand)
    require(motif.chirality == trace.roles[0], "motif/trace handedness differs")
    return (trace,certificate,registry),motif


def _prepared_wave(carrier):
    lineage = "retained-A" if carrier == 0 else "retained-B"
    components = tuple((t,native.certify(t)) for t in (
        native.trace(lineage,0,start_index=0,scope=SCOPE,chirality=1),
        native.trace(lineage,1,start_index=10,scope=SCOPE,chirality=-1)))
    axes = (0,1,2) if carrier == 0 else (2,1,0)
    word = WORDS.index(axes)
    records,motifs = [],[]
    for k in range(3 if carrier == 0 else 2):
        record,motif = _target(lineage,k+2,20+10*k,word,1 if carrier == 0 else -1,components)
        records.append(record)
        motifs.append(motif)
    wave = native.wave(records,motifs)
    native.validate_wave(wave)
    return wave,components


def _contact_pair(waves, tick, previous=None, common_mask=15):
    cut = 52 if tick == -1 else 70+18*tick
    lower = -1 if previous is None else previous.committed
    reflections = tuple(tuple(t.event_ids[t.turn] for t in w.returns
                              if lower < t.indices[-1] <= cut) for w in waves)
    counts = tuple(sum(t.indices[-1] <= cut for t in w.returns) for w in waves)
    require(common_mask > 0, "no shared contact support axis")
    axis = next(i for i in range(4) if common_mask & (1 << i))
    contacts = []
    for source in (0,1):
        a,b = waves[source],waves[1-source]
        prefix = tuple(t for t in a.returns if t.indices[-1] <= cut)
        require(prefix,"contact precedes source formation")
        trace = native.trace(a.identity+"|"+b.identity,tick+1,
            vertices=(0,1 << axis,0),start_index=cut-2,scope=SCOPE,
            chirality=prefix[-1].roles[0])
        certificate = native.certify(trace)
        identity = native.digest(("retained-contact",a.identity,b.identity,cut,trace.return_id))
        contacts.append(native.Contact(identity,a.identity,b.identity,SCOPE,cut,
            native.wave_prefix(a,cut),native.wave_prefix(b,cut),
            (len(reflections[source]),len(reflections[1-source])),trace,certificate))
    return ContactPair(native.digest(tuple(c.identity for c in contacts)),tick,cut,
                       tuple(contacts),reflections,counts)


def _relation(waves, previous, current, target):
    source = 1-target
    old,new = previous.contacts[source],current.contacts[source]
    # Exercise the corrected collection-admission boundary before using counts.
    native.compare(waves[source],waves[target],(old,new),window=(0,current.committed))
    rcd = native.RCD(waves[source].identity,waves[target].identity,SCOPE,
        (old.identity,new.identity),3,
        (len(previous.reflection_occurrences[source]),len(current.reflection_occurrences[source])))
    t = new.support_trace
    units = ("edge:"+t.event_ids[0]+":"+t.event_ids[1],
             "edge:"+t.event_ids[1]+":"+t.event_ids[2],"hinge:"+t.event_ids[1])
    units += tuple("reflection:"+i for i in previous.reflection_occurrences[source])
    units += tuple("reflection:"+i for i in current.reflection_occurrences[source])
    require(len(units) == rcd.duration(), "RCD units differ from their counted occurrences")
    clipped = units[:CLIP]
    left = tuple((trace,cert) for trace,cert in zip(waves[source].returns,waves[source].certificates)
                 if trace.indices[-1] <= current.committed)
    right = tuple((trace,cert) for trace,cert in zip(waves[target].returns,waves[target].certificates)
                  if trace.indices[-1] <= current.committed)
    delta = len(left)-len(right)
    tail = left[len(right):] if delta > 0 else right[len(left):]
    unmatched = tuple(cert.return_id for _,cert in tail)
    orientation = t.roles[0] * (1 if delta >= 0 else -1)
    identity = native.digest(("retained-relation",target,old.identity,new.identity,
                               clipped,unmatched,orientation))
    tokens = tuple(Opportunity(native.digest((identity,u,r)),orientation,"retained",
                               identity,u,r) for u in clipped for r in unmatched)
    value = sum(token.hand for token in tokens)
    require(value == t.roles[0]*min(CLIP,rcd.duration())*delta,
            "witness enumeration and signed contribution differ")
    return RetainedRelation(identity,target,current.tick,current.committed,
        current.committed+1,(old.identity,new.identity),rcd,clipped,unmatched,value,tokens)


def initialize_world():
    """Admit actual same-lineage 1:2 constructions and the initial shared contact."""
    prepared = tuple(_prepared_wave(i) for i in (0,1))
    waves = tuple(p[0] for p in prepared)
    components = tuple(p[1] for p in prepared)
    contact = _contact_pair(waves,-1)
    fields = tuple(native.field(SCOPE,"field-"+str(i),(wave,)) for i,wave in enumerate(waves))
    world = World(initial_state(),waves,components,(contact,),(),(),fields)
    require(extract_state(world) == initial_state(), "formation differs from compact initial state")
    return world


def extract_state(world):
    """Read the next compact key from actual complete records, not prior cached counts."""
    tick = len(world.attempts)
    waves = world.waves
    latest = world.contacts[-1]
    incoming = tuple(w.returns[-1].vertices[-2].bit_length()-1 for w in waves)
    signs = tuple(w.returns[-1].roles[0] for w in waves)
    counts = tuple(len(w.returns) for w in waves)
    since = tuple(sum(t.indices[-1] > latest.committed for t in w.returns) for w in waves)
    age = min(3,tick-1-latest.tick)
    refs = tuple(map(len,latest.reflection_occurrences))
    values = []
    for target in (0,1):
        for created in (tick-1,tick-2):
            matches = tuple(r for r in world.relations if r.target == target and r.created_tick == created
                            and r.identity not in world.disabled_relations)
            require(len(matches) <= 1, "duplicate retained relation slot")
            values.extend((matches[0].value,1) if matches else (0,0))
    return validate_state((tick,*incoming,*signs,*counts,*since,age,*refs,*values))


def inventory(world, carrier, mask):
    """Full provenance token/word inventory; scalar counts are checked against it."""
    require(carrier in (0,1) and type(carrier) is int,"carrier index required")
    _admit_records(world)
    state = extract_state(world)
    tokens = []
    for trace,cert in world.components[carrier]:
        tokens.append(Opportunity(native.digest(("formation",cert.return_id)),trace.roles[0],
                                  "formation",cert.return_id,"core-component",cert.return_id))
    for created in (state[0]-1,state[0]-2):
        for relation in world.relations:
            if (relation.target == carrier and relation.created_tick == created
                    and relation.identity not in world.disabled_relations):
                require(relation.retained_at < 60+18*state[0], "same-cut retained influence")
                tokens.extend(relation.tokens)
    rows = []
    for hand in (-1,1):
        chosen = tuple(t for t in tokens if t.hand == hand)
        for word in eligible_words(state[1+carrier],mask):
            rows.extend((hand,word,t) for t in chosen)
    return tuple(rows)


def _same(left,right):
    if type(left) is not type(right):
        return False
    if isinstance(left,(tuple,list)):
        return len(left)==len(right) and all(_same(a,b) for a,b in zip(left,right))
    if isinstance(left,dict):
        return left.keys()==right.keys() and all(_same(left[k],right[k]) for k in left)
    if is_dataclass(left):
        return all(_same(getattr(left,f.name),getattr(right,f.name)) for f in fields(left))
    return left==right


def _admit_records(world):
    """Rebind prior contact/reflection/R tokens before they influence selection."""
    for i,wave in enumerate(world.waves):
        native.validate_wave(wave)
        for motif in wave.motif_witnesses:
            require(_same(tuple(zip(motif.component_traces,motif.component_certificates)),world.components[i]),
                    "formation basis differs from retained motif components")
            require(motif.core=="1:2" and motif.enclosure==6 and
                    type(motif.enclosure) is int and motif.chirality==motif.support_trace.roles[0],
                    "invalid retained core/enclosure/handedness binding")
            t=motif.support_trace
            require(len(t.vertices)==7 and len(set(t.vertices[:-1]))==6 and t.turn==3,
                    "retained target is not the declared C6 realization")
    previous = None
    for pair in world.contacts:
        require(type(pair.tick) is int and -1 <= pair.tick < len(world.attempts),"contact attempt outside archive")
        common = 15 if pair.tick == -1 else world.attempts[pair.tick].masks[0]&world.attempts[pair.tick].masks[1]
        expected = _contact_pair(world.waves,pair.tick,previous,common)
        require(_same(expected,pair),"contact/reflection occurrences differ from complete histories")
        previous = pair
    for relation in world.relations:
        require(type(relation.target) is int and relation.target in (0,1),"invalid relation target")
        source = 1-relation.target
        pair_indices = [i for i,p in enumerate(world.contacts) if p.contacts[source].identity == relation.contact_ids[-1]]
        require(len(pair_indices)==1 and pair_indices[0]>0,"unbound repeated-contact relation")
        i=pair_indices[0]
        old,new = world.contacts[i-1:i+1]
        require(new.tick-old.tick <= WINDOW,"relation uses expired contact")
        expected = _relation(world.waves,old,new,relation.target)
        require(_same(expected,relation),"retained relation/token provenance differs from admitted histories")


def advance_world(world, masks, tickets, computed_row=None):
    """Execute selected returns, contacts and retention; return a new immutable world.

    Backend rows may supply any subset of scalar audit fields, but each supplied
    value is compared exactly before it is used for witness reconstruction.
    """
    require(type(masks) is tuple and len(masks)==2 and type(tickets) is tuple and len(tickets)==2,
            "two masks and addressed tickets required")
    validate_state(world.state)
    state = extract_state(world)
    require(state == world.state, "cached state differs from witness extraction")
    predicted = transition(state,*masks,*tickets)
    if computed_row is not None:
        require(type(computed_row) is dict and "next_state" in computed_row and "words" in computed_row,
                "backend row needs computed next state and word decisions")
        require(all(k in predicted and _same(v,predicted[k]) for k,v in computed_row.items()),
                "backend row differs from counted transition")
    row = predicted if computed_row is None else {**predicted,**computed_row}
    waves = list(world.waves)
    selected = []
    for carrier in (0,1):
        choices = inventory(world,carrier,masks[carrier])
        require(len(choices)==row["totals"][carrier], "enumerated inventory differs from scalar total")
        if not choices:
            selected.append(None)
            continue
        index = tickets[carrier]*len(choices)//TICKET_DENOMINATOR
        hand,word,token = choices[index]
        require((hand,word)==(row["hands"][carrier],row["words"][carrier]),
                "token enumeration differs from scalar selection")
        selected.append(token)
        old = waves[carrier]
        record,motif = _target(old.lineage,100+state[0],61+18*state[0],word,hand,world.components[carrier])
        records = tuple((t,c,{c.certificate_digest:c}) for t,c in zip(old.returns,old.certificates))+(record,)
        wave = native.wave(records,old.motif_witnesses+(motif,))
        require(wave.identity==old.identity and wave.returns[:-1]==old.returns and
                wave.certificates[:-1]==old.certificates and wave.motif_witnesses[:-1]==old.motif_witnesses,
                "new return rewrote retained construction")
        waves[carrier] = wave
    waves = tuple(waves)
    contacts = world.contacts
    relations = world.relations
    if row["contact"]:
        pair = _contact_pair(waves,state[0],contacts[-1],masks[0]&masks[1])
        if pair.tick-contacts[-1].tick <= WINDOW:
            new = tuple(_relation(waves,contacts[-1],pair,target) for target in (0,1))
            require(tuple((r.value,1) for r in new)==row["new_relations"],
                    "full relation tokens differ from compact update")
            relations += new
        contacts += (pair,)
    attempts = world.attempts+(Attempt(masks,tickets,row,tuple(selected)),)
    fields = tuple(native.field(SCOPE,"field-"+str(i),(wave,)) for i,wave in enumerate(waves))
    require(len(native.resolve_fields(fields))==2,"population identity changed")
    result = World(row["next_state"],waves,world.components,contacts,relations,attempts,fields,
                   world.disabled_relations,world.interventions)
    require(extract_state(result)==row["next_state"], "full update/extraction does not commute")
    return result


def validate_world(world):
    """Replay admission and every full update; no external transcript authentication claim."""
    validate_state(world.state)
    rebuilt = initialize_world()
    for tick in range(len(world.attempts)+1):
        for action,identity,when in world.interventions:
            if when == tick:
                rebuilt = remove_relation(rebuilt,identity) if action == "remove" else restore_relation(rebuilt,identity)
        if tick == len(world.attempts):
            break
        attempt = world.attempts[tick]
        rebuilt = advance_world(rebuilt,attempt.masks,attempt.tickets,attempt.row)
    require(_same(rebuilt,world),"world differs from its admitted replay")
    for wave in world.waves:
        native.validate_wave(wave)
        require(wave.motif=="1:2","wrong core")
        for trace,motif in zip(wave.returns,wave.motif_witnesses):
            require(motif.enclosure==6 and motif.chirality==trace.roles[0],"enclosure/handedness mismatch")
            require(len(trace.vertices)==7 and len(set(trace.vertices[:-1]))==6,"missing actual C6 trace")
    require(extract_state(world)==world.state,"state extraction differs")
    return True


def remove_relation(world, identity):
    """Structural intervention on R; original relation and closure archives persist."""
    require(any(r.identity==identity for r in world.relations),"unknown intervention relation")
    require(identity not in world.disabled_relations,"relation is already absent")
    result = replace(world,disabled_relations=world.disabled_relations+(identity,),
                     interventions=world.interventions+(("remove",identity,len(world.attempts)),))
    return replace(result,state=extract_state(result))


def restore_relation(world, identity):
    require(identity in world.disabled_relations,"relation is not structurally absent")
    result = replace(world,disabled_relations=tuple(i for i in world.disabled_relations if i!=identity),
                     interventions=world.interventions+(("restore",identity,len(world.attempts)),))
    return replace(result,state=extract_state(result))
