"""Application trace integration, source rebinding and immutable witnesses."""
import hashlib
import json
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest

from simulations import boundary_accounting as a
from simulations import proton_accounting as proton
from simulations import electron_wire_accounting as electron
from simulations import trace_applications as apps
from simulations.trace_observation import ERROR_CLASSES
from simulations.trace_store import TraceStore, TraceLimits, TraceIncomplete, TraceIntegrityError, run_manifest


class AccountingRecorderRoutingTests(unittest.TestCase):
    def produce(self):
        archive, ledger = a.ArchiveState(), a.RouteLedger()
        for cut, identity in ((1, 'A'), (2, 'B')):
            source = a.ContentRecord('source-' + identity, 'physical-' + identity,
                '1' * 64, cut, 0, 'burden', 1, ('support-' + identity,))
            archive = a.commit_archive(archive, cut, ((source, 'routed'),))
            route = a.RouteRecord('route-' + identity, source.identity, cut, 1, 'burden',
                (a.RouteLeg('leg-' + identity, 'exoshedding', 1, 'outside'),), '1' * 64)
            ledger = a.register_route(ledger, archive, route)
        ledger = a.receive_route(ledger, 'route-A', 'leg-A', 3, 'outside', 'receipt-A', '1' * 64)
        self.assertEqual([row['route_id'] for row in a.pending_routes(ledger)], ['route-B'])
        return a.receive_route(ledger, 'route-B', 'leg-B', 4, 'outside', 'receipt-B', '1' * 64)

    def test_interleaved_receipts_keep_actual_route_after_reopen(self):
        expected = self.produce()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace'
            manifest = run_manifest('interleaved-routes', source='fixture',
                kernel='declared exact accounting', preparation='two disjoint routed sources')
            with TraceStore(path, manifest) as store:
                with a.observe_commits(apps.AccountingRecorder(store, 'interleaved')):
                    self.assertEqual(self.produce(), expected)
            with TraceStore(path, read_only=True) as reopened:
                result = apps.replay_accounting(reopened)
                self.assertEqual(result['coverage'], 'complete')
                route_a, route_b, receipt_a, receipt_b = result['transitions']
                self.assertEqual(reopened.get(receipt_a['event_id'])['causal_parents'], [route_a['event_id']])
                self.assertEqual(reopened.get(receipt_b['event_id'])['causal_parents'], [route_b['event_id']])
                self.assertEqual([row['route_id'] for row in receipt_a['pending_routes']], ['route-B'])
                self.assertEqual(receipt_b['pending_routes'], [])

    def test_alternative_branches_can_reuse_route_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace'
            manifest = run_manifest('alternative-routes', source='fixture',
                kernel='declared exact accounting', preparation='nonadditive alternate windows')
            with TraceStore(path, manifest) as store:
                recorder = apps.AccountingRecorder(store, 'alternatives', alternatives=True)
                with a.observe_commits(recorder):
                    # Each alternative is one independent archive/route family.
                    for amount in (1, 2):
                        source = a.ContentRecord('source', 'physical', '1' * 64, 1, 0,
                            'burden', amount, ('support',))
                        archive = a.commit_archive(a.ArchiveState(), 1, ((source, 'routed'),))
                        route = a.RouteRecord('route', source.identity, 1, amount, 'burden',
                            (a.RouteLeg('leg', 'exoshedding', amount, 'outside'),), '1' * 64)
                        ledger = a.register_route(a.RouteLedger(), archive, route)
                        a.receive_route(ledger, 'route', 'leg', 2, 'outside', 'receipt', '1' * 64)
            with TraceStore(path, read_only=True) as reopened:
                result = apps.replay_accounting(reopened)
                route_1, receipt_1, route_2, receipt_2 = result['transitions']
                self.assertNotEqual(route_1['event_id'], route_2['event_id'])
                self.assertEqual(reopened.get(receipt_1['event_id'])['causal_parents'], [route_1['event_id']])
                self.assertEqual(reopened.get(receipt_2['event_id'])['causal_parents'], [route_2['event_id']])
                self.assertEqual(len(result['applications']['alternatives']['branches']), 2)


class ApplicationTraceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.tmp.name)
        cls.region_plain = apps.run_regions()
        cls.particle_plain = apps.run_particles(cls.root / 'plain')
        manifest = run_manifest('application-regression', source={'repository':'aod-dec'},
            kernel='existing-D0-native-current', preparation='bound-existing-applications', backend='host')
        with TraceStore(cls.root / 'trace', manifest) as store:
            cls.region_traced = apps.run_regions(store)
            cls.particle_traced = apps.run_particles(cls.root / 'traced', store)
        cls.store = TraceStore(cls.root / 'trace', read_only=True)

    @classmethod
    def tearDownClass(cls):
        cls.store.close()
        cls.tmp.cleanup()

    def test_region_passive_equality(self):
        self.assertEqual(self.region_plain, self.region_traced)

    def test_particle_passive_equality(self):
        strip = lambda x: {k:v for k,v in x.items() if k != 'witnesses'}
        self.assertEqual(strip(self.particle_plain), strip(self.particle_traced))
        for name in ('proton-accounting.json','proton-accounting-witnesses.json.gz','electron-wire.json'):
            self.assertEqual((self.root/'plain'/name).read_bytes(), (self.root/'traced'/name).read_bytes())

    def test_real_commit_capture_and_namespace(self):
        events = tuple(self.store.iter_events())
        self.assertEqual(sum(e['kind']=='region.commit' for e in events),8)
        self.assertEqual(sum(e['kind']=='application.accounting.commit' for e in events),8)
        self.assertEqual(sum(e['kind']=='application.accounting.route' for e in events),1)
        self.assertEqual(sum(e['kind']=='application.accounting.receipt' for e in events),1)
        self.assertFalse(any(e['kind'].startswith('core.') for e in events))

    def test_alternative_views_do_not_invent_causal_sequence(self):
        events=tuple(self.store.iter_events())
        alternatives=[e for e in events if e['kind']=='application.accounting.commit' and e['scope_id']=='proton-window-alternatives']
        self.assertEqual(len(alternatives),4)
        self.assertTrue(all(e['causal_parents']==[] for e in alternatives))
        views=[e for e in events if e['kind']=='detector.boundary_view']
        self.assertTrue(views)
        self.assertTrue(all(self.store.get(e['causal_parents'][0])['kind']=='region.commit' for e in views))

    def test_witness_retrieval_after_reopen(self):
        for locator in self.particle_traced['witnesses']:
            self.assertEqual(apps.retrieve_blob(self.store, locator), (self.root/'traced'/locator['name']).read_bytes())
        self.assertEqual(self.store.verify()['coverage'],'complete')

    def test_fine_query_budget_never_returns_digest_as_witness(self):
        locator=self.particle_traced['witnesses'][0]
        with self.assertRaises(TraceIncomplete):
            apps.retrieve_blob(self.store,locator,max_bytes=1)
        missing=dict(locator,sha256='0'*64)
        with self.assertRaises(TraceIncomplete):
            apps.retrieve_blob(self.store,missing)

    def test_malformed_retrieval_locator_is_rejected(self):
        locator=self.particle_traced['witnesses'][0]
        for change in ({'bytes':True}, {'parts':True}, {'parts':-1}, {'scope':''}, {'parts':locator['bytes']+1}):
            with self.subTest(change=change), self.assertRaises(TraceIntegrityError):
                apps.retrieve_blob(self.store,dict(locator,**change))

    def test_existing_moving_boundary_and_gate_results(self):
        maser, earth = self.region_traced['examples']
        self.assertEqual(maser['summary']['detected_output_outward'],['4','0','4','5'])
        self.assertEqual(maser['summary']['retained_channel_occurrences'],20)
        self.assertEqual(earth['summary']['water-shell'],['10','7','-2','13'])
        self.assertEqual(earth['summary']['universe'],['0','0','0','0'])
        row=earth['audits']['water-shell']['rows'][1]
        self.assertEqual((row['cross'],row['sweep'],row['shedding'],row['current']),('8','3','2','7'))

    def test_particle_obligations_and_alternative_windows_survive(self):
        result=self.particle_traced
        self.assertFalse(result['proton_alternatives_additive'])
        self.assertEqual(result['proton']['crossing'],['-6','-12','-18','-30'])
        self.assertEqual(result['proton_full_signed_shedding_current'],{'status':'unresolved','value':None})
        self.assertEqual(result['electron']['wire_flux'],['4','0','4','5'])
        self.assertEqual(result['electron']['later_current'],['4','4','8','13'])
        report=json.loads((self.root/'traced/proton-accounting.json').read_text())
        self.assertTrue(report['rows'][-1]['unresolved_signed_projection'])
        self.assertEqual(len(report['rows'][-1]['pending_before_receipt']),1)
        self.assertEqual(report['rows'][-1]['pending_after_receipt'],[])

    def test_source_rebind_is_real(self):
        actual=hashlib.sha256(Path(a.__file__).read_bytes()).hexdigest()
        self.assertEqual(actual,proton.ACCOUNTING_SHA)
        self.assertEqual(actual,electron.ACCOUNTING_SHA)
        self.assertEqual(actual,apps.regions.r.ACCOUNTING_SHA256)

    def test_actual_observables_bind_scope_and_all_error_classes(self):
        region = self.region_traced['observations']
        particles = self.particle_traced['observations']
        self.assertEqual(len(region), 14)
        self.assertEqual(len(particles), 11)
        self.assertEqual(region['maser/detected-output/outward']['value'], '13')
        self.assertEqual(region['maser/apparatus/current']['value'], '0')
        self.assertEqual(region['earth-water-moon/water-shell/current']['value'], '28')
        self.assertEqual(region['earth-water-moon/water-shell/current']['window'], [200, 204])
        self.assertEqual(particles['electron-wire/wire/current']['value'], '13')
        self.assertEqual(particles['electron-wire/electron/current']['value'], '-13')
        self.assertEqual(particles['electron-wire/union/current']['value'], '0')
        self.assertEqual(particles['electron-wire/wire/current']['window'], [52, 56])
        for key, row in {**region, **particles}.items():
            with self.subTest(key=key):
                self.assertEqual(set(row['errors']), set(ERROR_CLASSES))
                self.assertEqual(row['unit'], 'native-pressure-current')
                self.assertEqual(row['anchor']['kind'], 'native')
                self.assertIsNone(row['anchor']['si_conversion'])
                self.assertTrue(row['evidence'])
                self.assertIn('#sha256=', row['preparation'])
                self.assertIn('#sha256=', row['kernel'])
                self.assertEqual(row['errors']['model_discrepancy']['status'], 'unknown')
                self.assertIsNone(row['errors']['model_discrepancy']['bound'])
                for error in row['errors'].values():
                    self.assertEqual(error['unit'], row['unit'])
                    if error['status'] in ('unknown', 'not_applicable'):
                        self.assertIsNone(error['bound'])

    def test_proton_observables_do_not_complete_or_add_alternative_currents(self):
        rows = self.particle_traced['observations']
        for window, current in ((2, '-6'), (4, '-12'), (6, '-18'), (12, '-30')):
            prefix = 'proton/participation-window-' + str(window)
            contact = rows[prefix + '/contact-current']
            unresolved = rows[prefix + '/full-current']
            self.assertEqual(contact['value'], current)
            self.assertEqual(contact['scale']['participation_windows'], [window])
            self.assertEqual(unresolved['status'], 'unresolved')
            self.assertIsNone(unresolved['value'])
            self.assertEqual(unresolved['errors']['truncation_aggregation']['status'], 'unknown')
            self.assertIsNone(unresolved['errors']['truncation_aggregation']['bound'])

    def test_automatic_accounting_replay_validates_real_route_and_later_receipt(self):
        result = apps.replay_accounting(self.store)
        self.assertEqual(result['coverage'], 'complete')
        self.assertEqual(result['validated_accounting_events'], 10)
        self.assertEqual([row['kind'] for row in result['transitions']], ['validated_route', 'validated_receipt'])
        route, receipt = result['transitions']
        self.assertEqual(route['record']['surplus'], 6)
        self.assertEqual(route['record']['legs'][0]['kind'], 'exoshedding')
        self.assertEqual(len(route['pending_routes']), 1)
        self.assertEqual(receipt['pending_routes'], [])
        self.assertGreater(receipt['record']['cut'], route['record']['committed'])
        alternatives = result['applications']['proton-window-alternatives']['branches']
        self.assertEqual(len(alternatives), 4)
        self.assertTrue(all(row['pending_routes'] == [] for row in alternatives))
        self.assertEqual(apps.replay_accounting(self.store, scope='electron-wire-baseline')['transitions'], [])

    def test_replay_rejects_route_partition_causality_and_duplicate_consumption(self):
        events = [event for event in self.store.iter_events() if event['kind'].startswith('application.accounting.')]
        route = next(i for i, event in enumerate(events) if event['kind'].endswith('.route'))
        receipt = next(i for i, event in enumerate(events) if event['kind'].endswith('.receipt'))
        class StoredEvents:
            def __init__(self, rows): self.rows = rows
            def iter_events(self): return iter(self.rows)
            def status(self): return {'coverage': 'complete'}
        def wrong_amount(rows): rows[route]['payload']['record']['legs'][0]['amount'] = {'$ratio': [7, 1]}
        def same_cut(rows): rows[receipt]['payload']['record']['cut'] = rows[route]['payload']['record']['committed']
        def wrong_parent(rows): rows[receipt]['causal_parents'] = rows[route]['causal_parents']
        def wrong_destination(rows): rows[receipt]['payload']['record']['destination'] = 'another-destination'
        def duplicate(rows): rows.append(deepcopy(rows[receipt]))
        def array_alias(rows): rows[0]['payload']['record']['additions'][0][0]['support_ids'] = 'characters'
        for mutation in (wrong_amount, same_cut, wrong_parent, wrong_destination, duplicate, array_alias):
            rows = deepcopy(events)
            mutation(rows)
            with self.subTest(mutation=mutation.__name__), self.assertRaises(TraceIntegrityError):
                apps.replay_accounting(StoredEvents(rows))

    def test_accounting_replay_reports_budget_and_partial_coverage(self):
        with self.assertRaises(TraceIncomplete):
            apps.replay_accounting(self.store, max_events=1)
        with self.assertRaises(TraceIncomplete):
            apps.replay_accounting(self.store, max_bytes=1)
        events = [event for event in self.store.iter_events() if event['kind'].startswith('application.accounting.')]
        events = events[:next(i for i, event in enumerate(events) if event['kind'].endswith('.receipt'))]
        class PartialEvents:
            def iter_events(self): return iter(events)
            def status(self): return {'coverage': 'partial'}
        report = apps.replay_accounting(PartialEvents())
        self.assertEqual(report['coverage'], 'partial')
        pending = report['applications']['proton-window-alternatives']['branches'][-1]['pending_routes']
        self.assertEqual(len(pending), 1)

    def test_live_tau_and_orbit_claims_require_their_producers(self):
        gaps=apps.application_gaps()
        self.assertFalse(gaps['tau']['live_run'])
        self.assertEqual(gaps['tau']['status'],'implementation_incomplete')
        self.assertIn('Collision, cavity and sensor setup are inputs',gaps['tau']['reason'])
        self.assertEqual(gaps['tau']['historical_missing_burden']['role'],'historical comparator, not generated output')
        self.assertFalse(gaps['sun_mercury']['physical_trajectory'])

    def test_native_formation_snapshots_are_queryable(self):
        from simulations.trace_support import detect_particle_support
        report=detect_particle_support(self.store)
        self.assertEqual(report['coverage'],'complete')
        self.assertEqual(report['signatures'],{'1:2':15,'3:3':15,'Duon':2})
        self.assertEqual(report['observed_formations'],32)
        self.assertTrue(all(row['evidence'] for row in report['returns']))
        self.assertIsNone(report['application_classification']['value'])

    def test_fold_history_is_chunked_and_recoverable(self):
        events=[e for e in self.store.iter_events() if e['kind']=='evidence.native_witness']
        fold=next(e for e in events if e['payload'].get('kind')=='particle.native_fold')
        raw=apps.retrieve_blob(self.store,fold['payload']['locator'])
        self.assertEqual(json.loads(raw)['capture'],'validated-retained-snapshot')
        self.assertGreater(len(raw),262144)


class AccountingObserverTests(unittest.TestCase):
    def record(self):
        return a.ContentRecord('x','x','a'*64,1,0,'current',1,('x',))

    def test_passive_commit_and_error_order(self):
        events=[]
        plain=a.commit_archive(a.ArchiveState(),1,((self.record(),'active'),))
        with a.observe_commits(lambda *args: events.append(args)):
            traced=a.commit_archive(a.ArchiveState(),1,((self.record(),'active'),))
            with self.assertRaises(ValueError):
                a.commit_archive(traced,1,())
        self.assertEqual(plain,traced)
        self.assertEqual(len(events),1)
        self.assertEqual(events[0],('accounting.commit',plain.commits[-1]))

    def test_context_is_restored_and_nested_observer_is_local(self):
        outer=[];inner=[]
        with a.observe_commits(lambda *args:outer.append(args)):
            with a.observe_commits(lambda *args:inner.append(args)):
                a.commit_archive(a.ArchiveState(),1,((self.record(),'active'),))
            a.commit_archive(a.ArchiveState(),1,((self.record(),'active'),))
        a.commit_archive(a.ArchiveState(),1,((self.record(),'active'),))
        self.assertEqual((len(outer),len(inner)),(1,1))

    def test_storage_failure_stops_before_successor_is_returned(self):
        source=a.ArchiveState()
        def failed(*args):raise TraceIncomplete('disk full')
        with a.observe_commits(failed):
            with self.assertRaisesRegex(TraceIncomplete,'disk full'):
                a.commit_archive(source,1,((self.record(),'active'),))
        self.assertEqual(source,a.ArchiveState())

    def test_failed_region_capture_reports_typed_incomplete(self):
        manifest=run_manifest('region-capture-failure',source={'base':'test'},
            kernel='D0',preparation='existing-regions',backend='host')
        with tempfile.TemporaryDirectory() as directory:
            store=TraceStore(Path(directory)/'trace',manifest,
                limits=TraceLimits(max_event_bytes=1024))
            try:
                with self.assertRaisesRegex(TraceIncomplete,'region commit capture incomplete'):
                    apps.run_regions(store)
                self.assertEqual(store.status()['coverage'],'partial')
                self.assertEqual(store.status()['events'],0)
            finally:store.close()

    def test_region_io_observer_failure_remains_incomplete(self):
        manifest=run_manifest('region-io-failure',source={'base':'test'},
            kernel='D0',preparation='existing-regions',backend='host')
        with tempfile.TemporaryDirectory() as directory:
            store=TraceStore(Path(directory)/'trace',manifest)
            def failed(*args):raise OSError('injected disk failure')
            store.core_recorder=failed
            try:
                with self.assertRaisesRegex(TraceIncomplete,'injected disk failure'):
                    apps.run_regions(store)
                store.finish()
                self.assertEqual(store.status()['coverage'],'partial')
            finally:store.close()

if __name__=='__main__':unittest.main()
