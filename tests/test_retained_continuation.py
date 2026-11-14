"""Independent opportunity enumeration and retained-history update controls.

Expected selections enumerate the declared finite token/word set directly.
They do not call the application's geometry, counting, or selection helpers.
The witness checks below exercise actual complete returns independently of the
scalar transition comparison; neither establishes a physical collision law.
"""
from copy import deepcopy
from dataclasses import replace
from itertools import permutations
from math import lcm
import unittest

from simulations import retained_continuation as model


FIELDS = (
    "tick", "incoming_a", "incoming_b", "chi_a", "chi_b",
    "count_a", "count_b", "since_a", "since_b", "contact_age",
    "previous_ref_a", "previous_ref_b",
    "r0a", "p0a", "r1a", "p1a", "r0b", "p0b", "r1b", "p1b",
)
INITIAL = (0, 2, 0, 1, -1, 3, 2, 0, 0, 0, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0)
WORDS = tuple(permutations(range(4), 3))


def changed(state=INITIAL, **values):
    out = dict(zip(FIELDS, state))
    out.update(values)
    return tuple(out[key] for key in FIELDS)


def explicit_cycle(word):
    a, b, c = (1 << axis for axis in word)
    return (0, a, a ^ b, a ^ b ^ c, b ^ c, c, 0)


def provenance_tokens(state, carrier):
    fields = dict(zip(FIELDS, state))
    tokens = {-1: [("formation", 0)], 1: [("formation", 1)]}
    for age in (0, 1):
        value = fields[f"r{age}{carrier}"]
        if fields[f"p{age}{carrier}"]:
            for unit in range(abs(value)):
                tokens[1 if value > 0 else -1].append(("relation", age, unit))
    return tokens


def expanded_inventory(state, carrier, mask):
    """List every declared opportunity, retaining cancellation provenance."""
    fields = dict(zip(FIELDS, state))
    tokens = provenance_tokens(state, carrier)
    admitted = []
    for index, word in enumerate(WORDS):
        vertices = explicit_cycle(word)
        edge_axes = [(a ^ b).bit_length() - 1 for a, b in zip(vertices, vertices[1:])]
        if edge_axes[0] == fields[f"incoming_{carrier}"]:
            continue
        if any(not (mask & (1 << axis)) for axis in edge_axes):
            continue
        admitted.append(index)
    return [(hand, index, token) for hand in (-1, 1)
            for index in admitted for token in tokens[hand]]


def selection(inventory, ticket):
    if not inventory:
        return None
    # Direct indexing of the expanded finite uniform sample space, rather
    # than the application's cumulative grouped-weight selection algorithm.
    return inventory[ticket * len(inventory) // model.TICKET_DENOMINATOR]


class RetainedContinuationScalarTests(unittest.TestCase):
    def test_frozen_interface_and_exact_ticket_grid(self):
        self.assertEqual(tuple(model.STATE_FIELDS), FIELDS)
        self.assertEqual(model.initial_state(), INITIAL)
        self.assertEqual(model.HORIZON, 6)
        expected = 36 * lcm(*range(1, 87))
        self.assertEqual(model.TICKET_DENOMINATOR, expected)
        for geometry in (4, 6, 18):
            for opportunities in range(2, 87):
                self.assertEqual(expected % (geometry * opportunities), 0)
        self.assertEqual(18 * (2 + 42 + 42), 1548)

    def test_all_incoming_slots_masks_and_actual_six_edge_geometry(self):
        observed_counts = set()
        for incoming in range(4):
            for mask in range(16):
                state = changed(incoming_a=incoming)
                inventory = expanded_inventory(state, "a", mask)
                geometry = len(inventory) // 2
                observed_counts.add(geometry)
                for ticket in (0, model.TICKET_DENOMINATOR - 1):
                    with self.subTest(incoming=incoming, mask=mask, ticket=ticket):
                        row = model.transition(state, mask, 0, ticket, 0)
                        self.assertEqual(row["counts"], ((1, 1), (1, 1)))
                        self.assertEqual(row["geometry_counts"], (geometry, 0))
                        self.assertEqual(row["totals"], (len(inventory), 0))
                        expected = selection(inventory, ticket)
                        self.assertEqual(row["words"][1], -1)
                        self.assertEqual(row["completed"], (bool(inventory), False))
                        self.assertFalse(row["contact"])
                        if expected is None:
                            self.assertEqual(row["words"][0], -1)
                            self.assertEqual(row["next_state"][1:5], state[1:5])
                            self.assertEqual(row["next_state"][5:9], state[5:9])
                        else:
                            hand, index, _ = expected
                            self.assertEqual(row["words"][0], index)
                            self.assertEqual(row["next_state"][1], WORDS[index][-1])
                            self.assertEqual(row["next_state"][3], hand)
                            vertices = explicit_cycle(WORDS[index])
                            self.assertEqual(len(set(vertices[:-1])), 6)
                            self.assertEqual(vertices[-1], vertices[0])
                            self.assertTrue(all((a ^ b).bit_count() == 1
                                                for a, b in zip(vertices, vertices[1:])))
        self.assertEqual(observed_counts, {0, 4, 6, 18})

    def test_second_carrier_all_masks_and_incoming_slots(self):
        for incoming in range(4):
            for mask in range(16):
                state = changed(incoming_b=incoming)
                inventory = expanded_inventory(state, "b", mask)
                for ticket in (0, model.TICKET_DENOMINATOR - 1):
                    row = model.transition(state, 0, mask, 0, ticket)
                    expected = selection(inventory, ticket)
                    self.assertEqual(row["geometry_counts"], (0, len(inventory) // 2))
                    self.assertEqual(row["totals"], (0, len(inventory)))
                    self.assertEqual(row["completed"], (False, bool(inventory)))
                    self.assertEqual(row["words"][0], -1)
                    self.assertEqual(row["words"][1], -1 if expected is None else expected[1])
                    self.assertEqual(row["next_state"][4], state[4] if expected is None else expected[0])

    def test_every_expanded_opportunity_threshold_for_signed_retention(self):
        # All examples lie within the declared value bounds. Signs represent
        # different provenance families, including cancellation in the sum.
        settings = (
            {}, {"r0a": 6, "p0a": 1}, {"r0a": -6, "p0a": 1},
            {"r0a": 5, "p0a": 1, "r1a": -5, "p1a": 1},
            {"r0a": 0, "p0a": 1}, {"r1a": 5, "p1a": 1},
        )
        for retention in settings:
            state = changed(tick=2, count_a=5, count_b=4,
                            previous_ref_a=1, previous_ref_b=1, **retention)
            inventory = expanded_inventory(state, "a", 15)
            width = model.TICKET_DENOMINATOR // len(inventory)
            tokens = provenance_tokens(state, "a")
            minus, plus = len(tokens[-1]), len(tokens[1])
            for rank, (hand, index, _) in enumerate(inventory):
                for ticket in (rank * width, (rank + 1) * width - 1):
                    with self.subTest(retention=retention, rank=rank, ticket=ticket):
                        row = model.transition(state, 15, 0, ticket, 0)
                        self.assertEqual(row["counts"][0], (minus, plus))
                        self.assertEqual(row["words"][0], index)
                        self.assertEqual(row["next_state"][3], hand)

    def test_atomic_prefix_counts_are_pushforward_of_expanded_inventory(self):
        for incoming in range(4):
            state = changed(tick=2, count_a=5, count_b=4, incoming_a=incoming,
                            r0a=5, p0a=1, r1a=-2, p1a=1)
            for mask in (0, 7, 11, 15):
                inventory = expanded_inventory(state, "a", mask)
                prefixes = {(0,)} | {explicit_cycle(WORDS[index])[:length]
                    for _, index, _ in inventory for length in range(1, 8)}
                for prefix in prefixes:
                    for selected_hand in (None, -1, 1):
                        expected = {}
                        for hand, index, _ in inventory:
                            vertices = explicit_cycle(WORDS[index])
                            if (selected_hand is not None and selected_hand != hand
                                    or vertices[:len(prefix)] != prefix or len(prefix) == 7):
                                continue
                            edge = prefix[-1], vertices[len(prefix)]
                            expected[edge] = expected.get(edge, 0) + 1
                        self.assertEqual(model.atomic_prefix_counts(
                            state, 0, mask, prefix, selected_hand), expected)
                        self.assertTrue(all((a ^ b).bit_count() == 1 for a, b in expected))

    def test_cancelling_relations_retain_opportunities_and_zero_is_not_absence(self):
        none = changed(tick=2, count_a=5, count_b=4)
        cancel = changed(none, r0a=5, p0a=1, r1a=-5, p1a=1)
        zero = changed(none, r0a=0, p0a=1)
        baseline = model.transition(none, 15, 0, 0, 0)
        full = model.transition(cancel, 15, 0, 0, 0)
        retained_zero = model.transition(zero, 15, 0, 0, 0)
        self.assertEqual(baseline["counts"][0], (1, 1))
        self.assertEqual(full["counts"][0], (6, 6))
        self.assertEqual(baseline["totals"][0], 36)
        self.assertEqual(full["totals"][0], 216)
        self.assertEqual(retained_zero["counts"], baseline["counts"])
        self.assertEqual(retained_zero["next_state"][15], 1)
        self.assertEqual(baseline["next_state"][15], 0)
        self.assertNotEqual(retained_zero["next_state"], baseline["next_state"])

    def test_causal_retention_then_expiry_and_reseeding(self):
        state = INITIAL
        rows = []
        for mask in (15, 0, 0, 15, 15, 15):
            row = model.transition(state, mask, mask, 0, 0)
            rows.append(row)
            state = row["next_state"]
        self.assertEqual(rows[0]["counts"], ((1, 1), (1, 1)))
        self.assertEqual(rows[0]["new_relations"], ((6, 1), (-6, 1)))
        self.assertEqual(rows[1]["completed"], (False, False))
        self.assertEqual(rows[1]["next_state"][12:20], (0, 0, 6, 1, 0, 0, -6, 1))
        self.assertEqual(rows[2]["next_state"][12:20], (0,) * 8)
        self.assertEqual(rows[3]["counts"], ((1, 1), (1, 1)))
        self.assertTrue(rows[3]["contact"])
        self.assertEqual(rows[3]["new_relations"], ((0, 0), (0, 0)))
        self.assertEqual(rows[4]["new_relations"], ((5, 1), (-5, 1)))
        self.assertEqual(rows[5]["counts"], ((1, 6), (6, 1)))
        self.assertEqual(state[0], model.HORIZON)

    def test_newly_equal_return_counts_create_present_zero_relations(self):
        first = model.transition(INITIAL, 0, 15, 0, 0)
        self.assertEqual(first["next_state"][5:7], (3, 3))
        self.assertEqual(first["next_state"][9], 1)
        second = model.transition(first["next_state"], 15, 15, 0, 0)
        self.assertTrue(second["contact"])
        self.assertEqual(second["next_state"][5:7], (4, 4))
        self.assertEqual(second["new_relations"], ((0, 1), (0, 1)))
        self.assertEqual(second["next_state"][12:20], (0, 1, 0, 0, 0, 1, 0, 0))

    def test_exact_input_domains_and_horizon_reject_hostile_values(self):
        cases = [
            (INITIAL[:-1], 15, 15, 0, 0),
            (changed(incoming_a=4), 15, 15, 0, 0),
            (changed(chi_b=0), 15, 15, 0, 0),
            (changed(count_a=-1), 15, 15, 0, 0),
            (changed(since_a=-1), 15, 15, 0, 0),
            (changed(contact_age=4), 15, 15, 0, 0),
            (changed(r0a=43, p0a=1), 15, 15, 0, 0),
            (changed(p0a=2), 15, 15, 0, 0),
            (changed(tick=6), 15, 15, 0, 0),
        ]
        for coordinate in range(len(INITIAL)):
            state = list(INITIAL)
            state[coordinate] = bool(state[coordinate])
            cases.append((tuple(state), 15, 15, 0, 0))
        for bad in (-1, 16, True, 1.0):
            cases.append((INITIAL, bad, 15, 0, 0))
            cases.append((INITIAL, 15, bad, 0, 0))
        for bad in (-1, model.TICKET_DENOMINATOR, True, 1.0):
            cases.append((INITIAL, 15, 15, bad, 0))
            cases.append((INITIAL, 15, 15, 0, bad))
        for index, args in enumerate(cases):
            with self.subTest(index=index), self.assertRaises((TypeError, ValueError)):
                model.transition(*args)
        for prefix in ((False,), (0.0,), (-1,), (16,), (0, True)):
            with self.subTest(prefix=prefix), self.assertRaises((TypeError, ValueError)):
                model.atomic_prefix_counts(INITIAL, 0, 15, prefix)


class RetainedContinuationWitnessTests(unittest.TestCase):
    def assert_preparation(self, world):
        self.assertEqual(len(world.waves), 2)
        self.assertEqual(len({wave.identity for wave in world.waves}), 2)
        self.assertEqual(model.extract_state(world), world.state)
        for wave, components, field in zip(world.waves, world.components, world.fields):
            self.assertTrue(model.native.validate_wave(wave))
            self.assertEqual(wave.motif, "1:2")
            self.assertEqual(field.members, (wave.identity,))
            self.assertEqual(len(components), 2)
            self.assertEqual({trace.roles[0] for trace, _ in components}, {-1, 1})
            ids = tuple(certificate.return_id for _, certificate in components)
            self.assertEqual(len(set(ids)), 2)
            for trace, certificate in components:
                self.assertEqual(certificate.motif, "Monon")
                self.assertEqual((trace.scope, trace.scale, trace.lineage),
                                 (wave.scope, wave.scale, wave.lineage))
                self.assertLess(trace.indices[-1], wave.returns[0].indices[0])
            for trace, motif in zip(wave.returns, wave.motif_witnesses):
                self.assertEqual((motif.core, motif.enclosure), ("1:2", 6))
                self.assertEqual(motif.chirality, trace.roles[0])
                self.assertEqual(trace.turn, 3)
                self.assertEqual(trace.roles, (motif.chirality, 0, -motif.chirality))
                self.assertEqual(len(trace.vertices), 7)
                self.assertEqual(len(set(trace.vertices[:-1])), 6)
                self.assertEqual(trace.vertices[0], trace.vertices[-1])
                self.assertTrue(all((a ^ b).bit_count() == 1
                                    for a, b in zip(trace.vertices, trace.vertices[1:])))
                self.assertEqual(motif.construction, ((ids,), ids))
                self.assertEqual(motif.component_traces, tuple(t for t, _ in components))
                self.assertEqual(motif.component_certificates, tuple(c for _, c in components))

    def assert_contacts_and_relations_from_actual_occurrences(self, world):
        contacts_by_id = {}
        prior_cut = -1
        for pair in world.contacts:
            expected_hinges = tuple(tuple(trace.event_ids[trace.turn] for trace in wave.returns
                if prior_cut < trace.indices[-1] <= pair.committed) for wave in world.waves)
            self.assertEqual(pair.reflection_occurrences, expected_hinges)
            self.assertEqual(pair.prefix_counts, tuple(sum(trace.indices[-1] <= pair.committed
                for trace in wave.returns) for wave in world.waves))
            for source, contact in enumerate(pair.contacts):
                contacts_by_id[contact.identity] = (pair, source, contact)
                self.assertEqual(contact.reflection,
                                 (len(expected_hinges[source]), len(expected_hinges[1-source])))
                self.assertEqual(contact.support_trace.indices[-1], pair.committed)
                self.assertEqual(contact.support_trace.roles[1:-1], (0,))
            prior_cut = pair.committed
        for relation in world.relations:
            previous, source, old = contacts_by_id[relation.contact_ids[0]]
            current, new_source, new = contacts_by_id[relation.contact_ids[1]]
            self.assertEqual(source, new_source)
            self.assertEqual(source, 1 - relation.target)
            self.assertLess(previous.committed, current.committed)
            self.assertLessEqual(current.tick - previous.tick, 2)
            self.assertEqual(relation.input_cut, current.committed)
            self.assertGreater(relation.retained_at, relation.input_cut)
            self.assertGreater(relation.retained_at, 60 + 18 * current.tick)
            self.assertLess(relation.retained_at, 60 + 18 * (current.tick + 1))
            trace = new.support_trace
            units = ("edge:" + trace.event_ids[0] + ":" + trace.event_ids[1],
                     "edge:" + trace.event_ids[1] + ":" + trace.event_ids[2],
                     "hinge:" + trace.event_ids[1])
            units += tuple("reflection:" + key for key in previous.reflection_occurrences[source])
            units += tuple("reflection:" + key for key in current.reflection_occurrences[source])
            self.assertEqual(relation.rcd.duration(), len(units))
            self.assertEqual(relation.support_units, units[:6])
            source_returns = tuple(t.return_id for t in world.waves[source].returns
                                   if t.indices[-1] <= current.committed)
            target_returns = tuple(t.return_id for t in world.waves[relation.target].returns
                                   if t.indices[-1] <= current.committed)
            if len(source_returns) >= len(target_returns):
                unmatched = source_returns[len(target_returns):]
                direction = trace.roles[0]
            else:
                unmatched = target_returns[len(source_returns):]
                direction = -trace.roles[0]
            self.assertEqual(relation.unmatched_returns, unmatched)
            expected = tuple((unit, occurrence, direction)
                             for unit in units[:6] for occurrence in unmatched)
            actual = tuple((token.unit_id, token.return_id, token.hand) for token in relation.tokens)
            self.assertEqual(actual, expected)
            self.assertEqual(relation.value, sum(hand for _, _, hand in expected))
            self.assertEqual(len({token.identity for token in relation.tokens}), len(expected))
            self.assertTrue(all(token.source_id == relation.identity
                                and token.source_kind == "retained" for token in relation.tokens))

    def test_initial_two_actual_motifs_and_field_population(self):
        world = model.initialize_world()
        self.assertEqual(world.state, INITIAL)
        self.assertEqual(tuple(len(wave.returns) for wave in world.waves), (3, 2))
        self.assertEqual(len(world.contacts), 1)
        self.assertEqual(world.contacts[0].committed, 52)
        self.assertEqual(world.relations, ())
        self.assert_preparation(world)
        self.assert_contacts_and_relations_from_actual_occurrences(world)
        self.assertTrue(model.validate_world(world))

    def test_primary_six_attempt_world_commutes_through_expiry_and_new_response(self):
        world = model.initialize_world()
        identities = tuple(w.identity for w in world.waves)
        schedule = ((15, 15), (0, 15), (0, 15), (15, 15), (15, 15), (15, 15))
        rows = []
        for masks in schedule:
            before = world
            row = model.transition(before.state, *masks, 0, 0)
            world = model.advance_world(before, masks, (0, 0), computed_row=row)
            rows.append(row)
            self.assertEqual(world.state, row["next_state"])
            self.assertEqual(model.extract_state(world), world.state)
            self.assertEqual(tuple(w.identity for w in world.waves), identities)
            self.assertEqual(world.components, before.components)
            self.assertEqual(world.contacts[:len(before.contacts)], before.contacts)
            self.assertEqual(world.relations[:len(before.relations)], before.relations)
            for index, (old, new) in enumerate(zip(before.waves, world.waves)):
                count = len(old.returns)
                self.assertEqual(new.returns[:count], old.returns)
                self.assertEqual(new.certificates[:count], old.certificates)
                self.assertEqual(new.motif_witnesses[:count], old.motif_witnesses)
                self.assertEqual(len(new.returns) - count, int(row["completed"][index]))
                if not row["completed"][index]:
                    self.assertEqual(new, old)
            self.assert_preparation(world)
            self.assert_contacts_and_relations_from_actual_occurrences(world)
        self.assertEqual([row["new_relations"] for row in rows], [
            ((6, 1), (-6, 1)), ((0, 0), (0, 0)), ((0, 0), (0, 0)),
            ((0, 0), (0, 0)), ((-6, 1), (5, 1)), ((-5, 1), (5, 1)),
        ])
        self.assertEqual(rows[2]["next_state"][12:20], (0,) * 8)
        self.assertEqual(rows[4]["counts"], ((1, 1), (1, 1)))
        self.assertEqual(rows[5]["counts"], ((7, 1), (1, 6)))
        self.assertEqual(world.state[5:7], (7, 8))
        self.assertEqual(len(world.relations), 6)
        self.assertTrue(any(r.created_tick == 0 for r in world.relations))
        self.assertTrue(model.validate_world(world))

    def test_zero_relation_is_created_from_completed_equal_prefix_counts(self):
        world = model.advance_world(model.initialize_world(), (0, 15), (0, 0))
        world = model.advance_world(world, (15, 15), (0, 0))
        self.assertEqual(world.state[5:7], (4, 4))
        self.assertEqual(len(world.relations), 2)
        self.assertEqual(world.state[12:20], (0, 1, 0, 0, 0, 1, 0, 0))
        self.assertTrue(all(relation.tokens == () and relation.value == 0 for relation in world.relations))
        self.assert_contacts_and_relations_from_actual_occurrences(world)
        self.assertTrue(model.validate_world(world))

    def test_removal_restoration_changes_a_later_completed_handed_return(self):
        world = model.initialize_world()
        for masks in ((15, 15), (0, 15), (0, 15), (15, 15), (15, 15)):
            world = model.advance_world(world, masks, (0, 0))
        relation = next(r for r in world.relations if r.target == 0 and r.created_tick == 4)
        self.assertEqual(relation.value, -6)
        removed = model.remove_relation(world, relation.identity)
        restored = model.restore_relation(removed, relation.identity)
        self.assertEqual(removed.state[:12], world.state[:12])
        for name in ("waves", "components", "contacts", "relations", "fields"):
            self.assertEqual(getattr(removed, name), getattr(world, name))
            self.assertEqual(getattr(restored, name), getattr(world, name))
        self.assertEqual(restored.state, world.state)
        self.assertEqual(model.inventory(restored, 0, 15), model.inventory(world, 0, 15))
        tickets = (model.TICKET_DENOMINATOR // 2, 0)
        full = model.advance_world(world, (15, 15), tickets)
        omitted = model.advance_world(removed, (15, 15), tickets)
        recovered = model.advance_world(restored, (15, 15), tickets)
        self.assertEqual(full.waves[0].returns[-1].roles[0], -1)
        self.assertEqual(omitted.waves[0].returns[-1].roles[0], 1)
        self.assertEqual(full.waves, recovered.waves)
        self.assertEqual(full.state, recovered.state)
        self.assertEqual(full.waves[0].returns[:-1], omitted.waves[0].returns[:-1])
        for branch in (full, omitted, recovered):
            self.assertTrue(model.validate_world(branch))
        # Structural blockage prevents the altered opportunity inventory from
        # creating an A return; diagnostic counts may still differ.
        held_full = model.advance_world(world, (0, 15), tickets)
        held_omitted = model.advance_world(removed, (0, 15), tickets)
        self.assertEqual(held_full.waves, held_omitted.waves)
        self.assertEqual(held_full.waves[0], world.waves[0])
        self.assertEqual(held_full.attempts[-1].row["words"], held_omitted.attempts[-1].row["words"])
        self.assertEqual(held_full.attempts[-1].row["words"][0], -1)
        self.assertTrue(model.validate_world(held_omitted))

    def test_cancelling_actual_relations_keep_tokens_and_expire_individually(self):
        world = model.initialize_world()
        for ticket_b in (0, model.TICKET_DENOMINATOR - 1, 0):
            world = model.advance_world(world, (15, 15), (0, ticket_b))
        active = [r for r in world.relations if r.target == 0 and r.created_tick in (1, 2)]
        self.assertEqual(sorted(r.value for r in active), [-5, 5])
        self.assertEqual(sum(r.value for r in active), 0)
        self.assertEqual(sum(len(r.tokens) for r in active), 10)
        baseline = model.transition(world.state, 15, 15, 0, 0)
        self.assertEqual(baseline["counts"][0], (6, 6))
        positive = next(r for r in active if r.value > 0)
        negative = next(r for r in active if r.value < 0)
        no_positive = model.remove_relation(world, positive.identity)
        no_negative = model.remove_relation(world, negative.identity)
        self.assertEqual(model.transition(no_positive.state, 15, 15, 0, 0)["counts"][0], (6, 1))
        self.assertEqual(model.transition(no_negative.state, 15, 15, 0, 0)["counts"][0], (1, 6))
        empty = model.remove_relation(no_positive, negative.identity)
        self.assertEqual(model.transition(empty.state, 15, 15, 0, 0)["counts"][0], (1, 1))
        expired = model.advance_world(world, (0, 0), (0, 0))
        self.assertEqual(expired.state[12:16], (0, 0, 5, 1))
        self.assertEqual(expired.relations, world.relations)
        self.assertEqual(expired.waves, world.waves)
        self.assertEqual(model.transition(expired.state, 15, 15, 0, 0)["counts"][0], (1, 6))
        self.assert_contacts_and_relations_from_actual_occurrences(expired)
        self.assertTrue(model.validate_world(expired))

    def test_actual_zero_relation_removal_changes_presence_without_inventing_weight(self):
        world = model.advance_world(model.initialize_world(), (0, 15), (0, 0))
        world = model.advance_world(world, (15, 15), (0, 0))
        zero = next(r for r in world.relations if r.target == 0)
        self.assertEqual(zero.value, 0)
        removed = model.remove_relation(world, zero.identity)
        self.assertNotEqual(removed.state, world.state)
        self.assertEqual(model.inventory(removed, 0, 15), model.inventory(world, 0, 15))
        self.assertEqual(removed.relations, world.relations)
        restored = model.restore_relation(removed, zero.identity)
        self.assertEqual(restored.state, world.state)
        self.assertTrue(model.validate_world(removed))
        self.assertTrue(model.validate_world(restored))

    def test_invalid_backend_rows_and_changed_formation_fail_before_execution(self):
        world = model.initialize_world()
        row = model.transition(world.state, 15, 15, 0, 0)
        bad_rows = []
        altered = deepcopy(row)
        altered["next_state"] = changed(row["next_state"], count_a=5)
        bad_rows.append(altered)
        altered = deepcopy(row)
        altered["completed"] = (1, 1)
        bad_rows.append(altered)
        altered = deepcopy(row)
        altered["words"] = (-1, -1)
        bad_rows.append(altered)
        bad_rows.append({"words": row["words"]})
        for bad in bad_rows:
            with self.assertRaises((TypeError, ValueError)):
                model.advance_world(world, (15, 15), (0, 0), computed_row=bad)
            self.assertEqual(world.state, INITIAL)
            self.assertEqual(world.attempts, ())
        malformed = replace(world, components=(world.components[0][:-1], world.components[1]))
        with self.assertRaises((TypeError, ValueError)):
            model.validate_world(malformed)
        bad_wave = replace(world.waves[0], motif_witnesses=(
            replace(world.waves[0].motif_witnesses[0], enclosure=8), *world.waves[0].motif_witnesses[1:]))
        with self.assertRaises((TypeError, ValueError)):
            model.validate_world(replace(world, waves=(bad_wave, world.waves[1])))
        boolean_key = replace(world, state=(False,) + world.state[1:])
        with self.assertRaises((TypeError, ValueError)):
            model.validate_world(boolean_key)
        with self.assertRaises((TypeError, ValueError)):
            model.advance_world(boolean_key, (15, 15), (0, 0))

    def test_self_consistent_forged_tokens_and_same_count_reflections_fail_admission(self):
        world = model.advance_world(model.initialize_world(), (15, 15), (0, 0))
        relation = next(r for r in world.relations if r.target == 0)
        self.assertEqual(relation.value, 6)
        extra = replace(relation.tokens[0], identity="forged-extra-opportunity",
                        unit_id="forged-extra-support-unit", hand=1)
        forged_relation = replace(relation, value=relation.value + 1,
                                  tokens=relation.tokens + (extra,))
        forged = replace(world, relations=tuple(forged_relation if r.identity == relation.identity
                                               else r for r in world.relations))
        forged = replace(forged, state=model.extract_state(forged))
        self.assertEqual(sum(token.hand for token in forged_relation.tokens), forged_relation.value)
        self.assertEqual(len(forged_relation.tokens), 7)
        self.assertEqual(forged.state[12:14], (7, 1))
        computed = model.transition(forged.state, 15, 15, 0, 0)
        self.assertEqual(computed["counts"][0], (1, 8))
        # Cached counters, declared value and enumerated token count agree;
        # only reconstruction from actual support/return witnesses can reject.
        with self.assertRaises((TypeError, ValueError)):
            model.advance_world(forged, (15, 15), (0, 0), computed_row=computed)

        contact = world.contacts[-1]
        occurrences = contact.reflection_occurrences
        bad_first = ("forged-hinge-occurrence",) + occurrences[0][1:]
        bad_contact = replace(contact, reflection_occurrences=(bad_first, occurrences[1]))
        wrong_reflection = replace(world, contacts=world.contacts[:-1] + (bad_contact,))
        self.assertEqual(tuple(map(len, bad_contact.reflection_occurrences)),
                         tuple(map(len, occurrences)))
        self.assertEqual(model.extract_state(wrong_reflection), world.state)
        with self.assertRaises((TypeError, ValueError)):
            model.advance_world(wrong_reflection, (15, 15), (0, 0))

        self.assertEqual(contact.contacts[0].reflection, (1, 1))
        boolean_native_contact = replace(contact.contacts[0], reflection=(True, 1))
        boolean_pair = replace(contact, contacts=(boolean_native_contact, contact.contacts[1]))
        boolean_contact_world = replace(world, contacts=world.contacts[:-1] + (boolean_pair,))
        with self.assertRaises((TypeError, ValueError)):
            model.validate_world(boolean_contact_world)

        removed = model.remove_relation(world, relation.identity)
        self.assertEqual(removed.interventions[-1][2], 1)
        boolean_intervention = (*removed.interventions[-1][:2], True)
        boolean_intervention_world = replace(
            removed, interventions=removed.interventions[:-1] + (boolean_intervention,))
        with self.assertRaises((TypeError, ValueError)):
            model.validate_world(boolean_intervention_world)


if __name__ == "__main__":
    unittest.main()
