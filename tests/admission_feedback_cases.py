"""Strict admission case inventory from R04 feedback, reused for PR5 regression coverage.
Historical labels identify provenance; all non-positive cases must reject.
"""
from dataclasses import replace


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def with_middle_identity(trace, event_id):
    ids = (trace.event_ids[0], event_id, trace.event_ids[-1])
    return replace(trace, event_ids=ids,
                   reflection=replace(trace.reflection,
                                      turnaround_id=event_id,
                                      outbound_ids=(event_id,)))


def collection(m, conflicting=False):
    left = m.recurrent('left', n=3)
    right = m.recurrent('right', n=4, spacing=6)
    contacts = m.contacts(left, right)
    require(len(contacts) == 2, 'fixture needs exactly two contacts')
    if conflicting:
        first, second = contacts
        altered = with_middle_identity(
            replace(second.support_trace, vertices=(0, 2, 0)),
            first.support_trace.event_ids[1])
        require((first.support_trace.scope, first.support_trace.scale,
                 first.support_trace.lineage) ==
                (altered.scope, altered.scale, altered.lineage),
                'conflict must remain in the same scoped identity domain')
        require((first.support_trace.vertices[1], first.support_trace.indices[1]) !=
                (altered.vertices[1], altered.indices[1]),
                'fixture must actually conflict')
        contacts = (first, replace(second, support_trace=altered,
                                   support_certificate=m.certify(altered)))
    return left, right, contacts


def cases(m):
    def ordinary_trace():
        record = m.certified(m.trace('positive'))
        m.admitted(*record)

    def shifted_integer_trace():
        # This producer declares integer cuts without an origin/nonnegative
        # restriction. (-1,0,1) gives terminal cut 1 for the Boolean-alias test.
        record = m.certified(m.trace('shifted', start_index=-1))
        require(record[0].indices == (-1, 0, 1), 'shifted exact-index control')
        m.admitted(*record)

    def valid_wave():
        m.validate_wave(m.recurrent('positive-wave', n=3))

    def identical_reuse():
        record = m.certified(m.trace('same-record'))
        require(m.phase_count((record, record)) == 1,
                'identical certified reuse must count one distinct closure')

    def scoped_reuse():
        first = m.trace('first-lineage')
        second = replace(first, lineage='other-lineage', ancestor='other:ancestor',
                         reflection=replace(first.reflection, lineage='other-lineage'))
        require(m.phase_count((m.certified(first), m.certified(second))) == 2,
                'event strings under distinct lineages must remain independent')

    def recurrence_conflict():
        first = m.trace('collision', 0)
        second = with_middle_identity(m.trace('collision', 1, vertices=(0, 2, 0)),
                                      first.event_ids[1])
        m.wave((m.certified(first), m.certified(second)))

    def contact_compare(conflicting):
        left, right, contacts = collection(m, conflicting)
        result = m.compare(left, right, contacts, window=(0, 23))
        require(result['duration'] == 5, 'valid comparison duration changed')

    def contact_accessor_conflict():
        _, _, contacts = collection(m, True)
        records = tuple((event.support_trace, event.support_certificate,
                         {event.support_certificate.certificate_digest:
                          event.support_certificate}) for event in contacts)
        m.phase_count(records)

    def reflection_alias(field, value, shifted=False):
        trace = m.trace('reflection-alias', start_index=-1 if shifted else 0)
        m.certified(replace(trace, reflection=replace(trace.reflection, **{field: value})))

    def missing_reflection():
        m.certified(replace(m.trace('missing'), reflection=None))

    def wrong_reflection_lineage():
        trace = m.trace('bound-lineage')
        m.certified(replace(trace, reflection=replace(trace.reflection, lineage='wrong')))

    # Each entry is (stable ID, historical behavior class, callable).
    return [
        ('POS-01-valid-trace', 'positive', ordinary_trace),
        ('POS-02-shifted-integer-cuts', 'positive', shifted_integer_trace),
        ('POS-03-valid-recurrence', 'positive', valid_wave),
        ('POS-04-identical-record-reuse', 'positive', identical_reuse),
        ('POS-05-distinct-lineage-name-reuse', 'positive', scoped_reuse),
        ('POS-06-distinct-valid-contact-supports', 'positive', lambda: contact_compare(False)),
        ('R04-01-recurrence-event-conflict', 'r04_repaired', recurrence_conflict),
        ('FIX-01-contact-collection-conflict', 'residual', lambda: contact_compare(True)),
        ('R04-02-same-contact-records-accessor', 'r04_repaired', contact_accessor_conflict),
        ('R04-03-boolean-vertex', 'r04_repaired', lambda: m.certified(m.trace(vertices=(0, True, 0)))),
        ('R04-04-boolean-chirality', 'r04_repaired', lambda: m.certified(m.trace(chirality=True))),
        ('R04-05-boolean-fibre-role', 'r04_repaired', lambda: m.fibre_step(True, 0, 'enter_hinge')),
        ('R04-06-boolean-window', 'r04_repaired', lambda: m.participation(
            m.RCD('A', 'B', 'B0', ('c1', 'c2'), 3, (1, 1)), True, 'native-return-count')),
        ('FIX-02-boolean-turnaround-cut', 'residual', lambda: reflection_alias('turnaround_committed_at', True)),
        ('FIX-03-float-turnaround-cut', 'residual', lambda: reflection_alias('turnaround_committed_at', 1.0)),
        ('FIX-04-boolean-terminal-cut', 'residual', lambda: reflection_alias('committed_at', True, True)),
        ('FIX-05-float-terminal-cut', 'residual', lambda: reflection_alias('committed_at', 2.0)),
        ('NEG-01-missing-reflection', 'negative', missing_reflection),
        ('NEG-02-wrong-reflection-lineage', 'negative', wrong_reflection_lineage),
    ]
