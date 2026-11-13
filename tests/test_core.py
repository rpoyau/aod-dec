import unittest
from dataclasses import replace
from fractions import Fraction
from itertools import product
from runtime import dec_core as c


def closed(lineage='a', scope='B', n=1, leading=0):
    t=c.new_trace(scope,lineage,c.Q4(0))
    for _ in range(leading):t=c.append_event(t,'dwell')
    for _ in range(n):
        t=c.append_event(t,'step',c.Q4(1),1)
        t=c.append_event(t,'hinge')
        t=c.append_event(t,'step',c.Q4(0),-1)
        t=c.close_trace(t)
    return t


class TernaryTests(unittest.TestCase):
    def test_t1_domain(self):
        for x in (-1,0,1):self.assertEqual(c.T1(x).value,x)
        for x in (-2,2,True,1.0):
            with self.assertRaises(c.DecError):c.T1(x)

    def test_all_single_lane_encodings(self):
        for lane, digit in product(range(32),(-1,0,1)):
            t=c.T32(digit*3**lane)
            self.assertEqual(c.T32.from_bits(t.bits()),t)
            self.assertEqual(c.apply_unary('-',c.apply_unary('-',t)),t)
            self.assertEqual(c.T32.from_bits(c.T32(-t.value).bits()),c.T32(-t.value))

    def test_t32_carries_and_boundaries(self):
        self.assertEqual(c.apply_binary('+',c.T32(1),c.T32(1)),c.T32(2))
        self.assertEqual(c.T32(2).bits(),0b0110)
        for n in (-c.T32.LIMIT,c.T32.LIMIT,0,1,-1):
            self.assertEqual(c.T32.from_bits(c.T32(n).bits()),c.T32(n))
        with self.assertRaises(c.DecError):c.apply_binary('+',c.T32(c.T32.LIMIT),c.T32(1))
        with self.assertRaises(c.DecError):c.apply_binary('+',c.T1(1),c.T1(1))

    def test_invalid_packed_values(self):
        for lane in range(32):
            with self.assertRaises(c.DecError):c.T32.from_bits(3<<(lane*2))
        for value in (-1,2**64,True):
            with self.assertRaises(c.DecError):c.T32.from_bits(value)

    def test_q4_domain(self):
        for n in range(16):self.assertEqual(c.Q4(n).value,n)
        for n in (-1,16,17,True):
            with self.assertRaises(c.DecError):c.Q4(n)

    def test_exact_large_fraction_addition(self):
        a=c.call_builtin('ratio',[1,4_000_000_000])
        self.assertEqual(c.apply_binary('+',a,a),Fraction(1,2_000_000_000))
        self.assertEqual(c.call_builtin('ratio',[2,-2**63]),Fraction(-1,2**62))
        with self.assertRaises(c.DecError):c.call_builtin('ratio',[1,0])
        with self.assertRaises(c.DecError):c.call_builtin('int',[Fraction(1,2)])

    def test_explicit_coercions_only(self):
        with self.assertRaises(c.DecError):c.apply_binary('+',1,c.T32(1))
        with self.assertRaises(c.DecError):c.apply_binary('+',c.Q4(1),c.Q4(1))
        self.assertEqual(c.call_builtin('int',[c.T32(-12)]),-12)
        self.assertEqual(c.apply_binary('/',c.T32(1),c.T32(2)),Fraction(1,2))


class ClosureTests(unittest.TestCase):
    def test_return_requires_start_hinge_and_opposition(self):
        start=c.new_trace('B','a',c.Q4(0))
        moved=c.append_event(start,'step',c.Q4(1),1)
        with self.assertRaises(c.DecError):c.close_trace(moved)
        with self.assertRaises(c.DecError):c.append_event(moved,'step',c.Q4(0),-1)
        hinged=c.append_event(moved,'hinge')
        with self.assertRaises(c.DecError):c.append_event(hinged,'step',c.Q4(0),1)
        returned=c.append_event(hinged,'step',c.Q4(0),-1)
        final=c.close_trace(returned)
        self.assertEqual((c.phases(final),c.beats(final)),(1,0))
        self.assertEqual(c.close_trace(final),final)
        self.assertEqual(c.phases(start),0)

    def test_atomic_domain_and_hinge(self):
        t=c.new_trace('B','a',c.Q4(0))
        with self.assertRaises(c.DecError):c.append_event(t,'step',c.Q4(3),1)
        with self.assertRaises(c.DecError):c.append_event(t,'step',c.Q4(0),1)
        with self.assertRaises(c.DecError):c.append_event(t,'hinge')
        with self.assertRaises(c.DecError):c.append_event(t,'step',c.Q4(1),0)

    def test_distinct_returns_and_dwell(self):
        t=closed(n=2)
        self.assertEqual((c.phases(t),c.beats(t)),(2,1))
        self.assertEqual(c.rate(t),Fraction(1,3))
        d=c.append_event(t,'dwell')
        self.assertEqual(c.phases(d),2)
        with self.assertRaises(c.DecError):c.close_trace(d)

    def test_record_tampering_and_replay_fail(self):
        t=closed()
        for mutant in [replace(t,scope='other'),replace(t,lineage='other'),replace(t,start=c.Q4(2)),
                       replace(t,closures=t.closures+t.closures),replace(t,events=t.events[:-1]),
                       replace(t,closures=(replace(t.closures[0],digest='invented'),))]:
            with self.assertRaises(c.DecError):c.validate_trace(mutant)

    def test_boolean_aliases_cannot_forge_event_or_certificate_fields(self):
        t=closed()
        mutants=[replace(t,closures=(replace(t.closures[0],begin=False),)),
                 replace(t,events=(replace(t.events[0],source=False),)+t.events[1:]),
                 replace(t,events=t.events[:1]+(replace(t.events[1],orientation=False),)+t.events[2:])]
        for mutant in mutants:
            with self.assertRaises(c.DecError):c.validate_trace(mutant)

    def test_cancellation_preserves_events(self):
        t=closed()
        self.assertEqual(sum(e.orientation for e in t.events),0)
        self.assertEqual(tuple(e.orientation for e in t.events),(1,0,-1))
        self.assertEqual(tuple(e.ordinal for e in t.events),(1,2,3))
        self.assertEqual(c.phases(t),1)

    def test_alternate_closed_route_not_forced_to_retrace(self):
        t=c.new_trace('B','a',c.Q4(0))
        t=c.append_event(t,'step',c.Q4(1),1)
        t=c.append_event(t,'step',c.Q4(3),1)
        t=c.append_event(t,'hinge')
        t=c.append_event(t,'step',c.Q4(2),-1)
        t=c.append_event(t,'step',c.Q4(0),-1)
        self.assertEqual(c.phases(c.close_trace(t)),1)


class FusionTests(unittest.TestCase):
    def test_outer_proposal_is_not_closure(self):
        a,b=closed('a'),closed('b')
        f=c.fold(a,b,'outer','ab')
        out=c.outer(f,c.Q4(0))
        self.assertEqual(c.phases(out),0)
        with self.assertRaises(c.DecError):c.fuse(f,out)
        out=c.append_event(out,'step',c.Q4(1),1)
        out=c.append_event(out,'hinge')
        out=c.append_event(out,'step',c.Q4(0),-1)
        out=c.close_trace(out)
        self.assertEqual(c.fuse(f,out).parents,(a,b))
        self.assertEqual(c.phases(out),1)
        self.assertEqual(c.phases(out.parents[0]),1)

    def test_raw_same_scope_outer_trace_cannot_bypass_fold(self):
        a,b=closed('a'),closed('b')
        forged=c.Trace('B','ab',c.Q4(0),parents=(a,b),proposal=c.fusion_digest(a,b,'B','ab'))
        with self.assertRaises(c.DecError):c.validate_trace(forged)

    def test_missing_or_mismatched_witness(self):
        a,b=closed('a'),closed('b'); f=c.fold(a,b,'outer','ab')
        with self.assertRaises(c.DecError):c.fold(a,a,'outer','aa')
        with self.assertRaises(c.DecError):c.fold(a,c.new_trace('B','b',c.Q4(0)),'outer','ab')
        with self.assertRaises(c.DecError):c.fold(a,b,'B','ab')
        with self.assertRaises(c.DecError):c.fuse(f,closed('ab','outer'))
        with self.assertRaises(c.DecError):c.outer(replace(f,digest='fake'),c.Q4(0))


class CouplingTests(unittest.TestCase):
    def test_repeated_contacts_lag_and_rate(self):
        a=c.append_event(closed('a',n=2),'dwell')
        b=closed('b',n=2,leading=1)
        cc=c.coupling(a,b,'window-0-7')
        with self.assertRaises(c.DecError):c.cadence(cc)
        cc=c.contact(cc,3,4)
        with self.assertRaises(c.DecError):c.cadence(cc)
        with self.assertRaises(c.DecError):c.contact(cc,3,4)
        with self.assertRaises(c.DecError):c.contact(cc,5,7)
        cc=c.contact(cc,6,7)
        self.assertEqual(c.cadence(cc),Fraction(1))
        self.assertEqual((c.lag(cc),c.drift(cc)),(-1,0))
        reverse=c.contact(c.contact(c.coupling(b,a,'window-0-7'),4,3),7,6)
        self.assertEqual(c.lag(reverse),1)
        self.assertEqual(c.cadence(reverse),1/c.cadence(cc))

    def test_scope_window_and_recurrence_requirements(self):
        a,b=closed('a'),closed('b')
        with self.assertRaises(c.DecError):c.coupling(a,a,'w')
        with self.assertRaises(c.DecError):c.coupling(a,closed('b','other'),'w')
        with self.assertRaises(c.DecError):c.coupling(a,closed('b',n=2),'w')
        with self.assertRaises(c.DecError):c.coupling(a,b,'')
        single=c.contact(c.coupling(a,b,'w'),3,3)
        with self.assertRaises(c.DecError):c.cadence(single)


class ContactImmutabilityTests(unittest.TestCase):
    def test_mutable_contact_record_is_rejected(self):
        pair=c.coupling(closed('a',n=2),closed('b',n=2),'w')
        forged=replace(pair,contacts=([3,3],(6,6)))
        with self.assertRaises(c.DecError):c.cadence(forged)
        good=c.contact(c.contact(pair,3,3),6,6)
        self.assertEqual(c.cadence(good),1)

class AFCTests(unittest.TestCase):
    def test_nonnegative_normalization_and_roles(self):
        self.assertEqual(c.normalize_weights((1,0,2)),(Fraction(1,3),0,Fraction(2,3)))
        self.assertEqual(c.call_builtin('probability',[1,0,2,c.T1(1)]),Fraction(2,3))
        for w in ((-1,1,3),(0,0,0),(1.0,1,1)):
            with self.assertRaises(c.DecError):c.normalize_weights(w)

    def test_region_iterator_cannot_silently_exhaust(self):
        self.assertEqual(c.finite_afc([1,0],[[0,1],[1,0]],[0])['boundary'],1)
        with self.assertRaises(c.DecError):c.finite_afc([1,0],[[0,1],[1,0]],iter([0]))

    def test_finite_stokes_all_regions(self):
        p=(Fraction(1,2),Fraction(1,3),Fraction(1,6))
        P=((0,Fraction(1,2),Fraction(1,2)),(1,0,0),(0,1,0))
        for bits in range(8):
            R=[i for i in range(3) if bits & (1<<i)]
            data=c.finite_afc(p,P,R)
            self.assertEqual(sum(data['divergence'][i] for i in R),data['boundary'])
            for i,j in product(range(3),repeat=2):self.assertEqual(data['flux'][i][j],-data['flux'][j][i])
        with self.assertRaises(c.DecError):c.finite_afc((1,0),((1,0),(-1,2)),(0,))
        with self.assertRaises(c.DecError):c.finite_afc((1,1),((1,0),(0,1)),(0,))

if __name__=='__main__':unittest.main()
