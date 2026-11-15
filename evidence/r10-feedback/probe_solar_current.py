from pathlib import Path
from dataclasses import replace
from fractions import Fraction as Q
import sys,json,tempfile,hashlib
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'source'))
from simulations import solar_boundary_fixture as s
from simulations import region_engine as r
from simulations.trace_store import TraceStore,run_manifest
n=s.n

def route_wave(source):
    pivot=(source.returns[-1],source.certificates[-1]);records=[];motifs=[]
    for index in range(3):
        t=n.trace(source.lineage,100+index,scope=s.SCOPE,start_index=25+6*index,chirality=1)
        c=n.certify(t);registry={x.certificate_digest:x for x in (pivot[1],c)}
        construction=((1,-1),(pivot[1].certificate_digest,1),(-1,1))
        m=n.motif_witness('Duon',t,construction,(pivot,),registry,enclosure=6,chirality=1)
        records.append((t,c,registry));motifs.append(m)
    return n.wave(tuple(records),tuple(motifs))

def probe(directory,variant):
    original=s.run();sun,mercury,route,receiver=original['native_waves']
    if variant:
        route=route_wave(sun)
        receiver=s._duon('solar-role:route-receiver',(1,)*6,start=22)
    n._validate_wave_collection((sun,mercury,route,receiver))
    route_evidence=dict(original['route_evidence'],route_support=route,receiver=receiver)
    fold=route_evidence['fold'];record=route_evidence['shedding'];cut=40
    comparison=s._comparison(route,receiver,bias=(Q(1),Q(0)))
    channel,operand=s._channel(comparison,cut,'solar-routed-current:40','solar-shell','exterior',Q(1,6),kind='shedding')
    operand.update(route_record=record.record_digest,surplus_to_current_map=None)
    channel=replace(channel,provenance=r.fingerprint(operand))
    pending=r.PendingProjection('solar-surplus-unmapped','solar-surplus:'+record.record_digest,'solar-shell','exterior',Q(3),s.BURDEN_UNIT,record.record_digest)
    path=directory/('variant' if variant else 'control')
    with TraceStore(path,run_manifest('solar-current-probe-'+str(variant),source=s.SOURCE,kernel='native scoped current',preparation='declared retained route comparison')) as store:
        for wave,family in ((sun,'solar-role'),(mercury,'mercury-role'),(route,'sheddic-E2-role'),(receiver,'route-receiver-role')):
            s._capture_wave(store,wave,family)
        outer=fold['outer'][0]
        s._append(store,'particle.native_fold',{'schema':'native-particle-fold-v1','capture':'validated-retained-snapshot','scope':outer.scope,'lineage':outer.lineage,'scale':outer.scale,'fold':fold},scope=outer.scope,lineage=outer.lineage,scale=1,key=outer.return_id)
        s._append(store,'evidence.solar_shedding',route_evidence,key=record.record_digest)
        ledger=r.Ledger(s.CELLS,s.UNIT,0,s.SCOPE,s.DOMAIN)
        with r.observe_commits(store.core_recorder):
            ledger=r.commit(ledger,cut,4,(channel,),obligations=(pending,))
        parent=store.get_by_key(json.dumps(['region',s.SCOPE,cut]))
        s._append(store,'evidence.channel_preparation',operand,key=operand['identity'],parents=(parent['event_id'],))
        s._append(store,'evidence.solar_chiral_flips',{'flips':original['chiral_flips'],'law':'prescribed whole-return signs; no transition probability'},key='whole-return-flips')
    with TraceStore.open(path) as store:
        result=s.inspect_trace(store)
        archive=store.status()
    return {'variant':variant,'native_route_valid':n.validate_wave(route),'native_receiver_valid':n.validate_wave(receiver),'route_cadence':route.cadence,'route_completion_cuts':[t.indices[-1] for t in route.returns],'receiver_completion_cuts':[t.indices[-1] for t in receiver.returns],'lag':str(comparison['comparison']['lag']),'calculated_current':str(channel.value),'reported_current':str(result['declared_routed_current']),'archive_coverage':archive['coverage'],'inspection':r.serial(result),'trace_path':str(path)}

if __name__=='__main__':
    directory=Path(sys.argv[1]);directory.mkdir(parents=True,exist_ok=True)
    result=[probe(directory,variant) for variant in (False,True)]
    print(json.dumps(result,indent=2))
