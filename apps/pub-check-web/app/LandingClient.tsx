'use client';

import { upload } from '@vercel/blob/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { COPY, COUNTRY_OPTIONS, LOCALE_OPTIONS, type SupportedCountry, type SupportedLocale } from '../lib/i18n';
import type { CheckResult } from '../lib/checks';
import type { CanonicalState } from '../lib/canonical-report';

const MAX_BYTES = 64 * 1024 * 1024;
type PublicStatus = {
  status: 'queued' | 'processing' | 'complete' | 'failed';
  emailStatus: 'pending' | 'sent' | 'failed' | 'not_configured';
  result?: CheckResult;
};
const statusLabels: Record<SupportedLocale, Record<PublicStatus['status'], string>> = {
  'en-US': {queued:'Queued',processing:'Processing',complete:'Complete',failed:'Failed'},
  'en-GB': {queued:'Queued',processing:'Processing',complete:'Complete',failed:'Failed'},
  'fr-FR': {queued:'En attente',processing:'Analyse',complete:'Terminé',failed:'Échec'},
  'es-ES': {queued:'En cola',processing:'Procesando',complete:'Completado',failed:'Error'},
  'it-IT': {queued:'In coda',processing:'In verifica',complete:'Completato',failed:'Errore'},
  'de-DE': {queued:'Warteschlange',processing:'Prüfung läuft',complete:'Fertig',failed:'Fehler'},
  'ru-RU': {queued:'В очереди',processing:'Проверяем',complete:'Готово',failed:'Ошибка'},
};
const compatibilityLabels: Record<SupportedLocale, Record<'compatible' | 'partial' | 'unsupported' | 'invalid' | 'failed', string>> = {
  'en-US': {compatible:'Compatible',partial:'Partial',unsupported:'Unsupported',invalid:'Invalid file',failed:'Check failed'},
  'en-GB': {compatible:'Compatible',partial:'Partial',unsupported:'Unsupported',invalid:'Invalid file',failed:'Check failed'},
  'fr-FR': {compatible:'Compatible',partial:'Partiel',unsupported:'Non pris en charge',invalid:'Fichier invalide',failed:'Échec'},
  'es-ES': {compatible:'Compatible',partial:'Parcial',unsupported:'No compatible',invalid:'Archivo no válido',failed:'Error'},
  'it-IT': {compatible:'Compatibile',partial:'Parziale',unsupported:'Non supportato',invalid:'File non valido',failed:'Errore'},
  'de-DE': {compatible:'Kompatibel',partial:'Teilweise',unsupported:'Nicht unterstützt',invalid:'Ungültige Datei',failed:'Fehler'},
  'ru-RU': {compatible:'Совместим',partial:'Частично',unsupported:'Не поддерживается',invalid:'Некорректный файл',failed:'Ошибка проверки'},
};

const canonicalLabels: Record<SupportedLocale, Record<CanonicalState, string>> = {
  'en-US': {opens_normally:'Opens normally',needs_review:'Needs review',opens_with_salvage:'Opens with recovery',unsupported:'Unsupported'},
  'en-GB': {opens_normally:'Opens normally',needs_review:'Needs review',opens_with_salvage:'Opens with recovery',unsupported:'Unsupported'},
  'fr-FR': {opens_normally:'S’ouvre normalement',needs_review:'À vérifier',opens_with_salvage:'Ouverture avec récupération',unsupported:'Non pris en charge'},
  'es-ES': {opens_normally:'Se abre normalmente',needs_review:'Requiere revisión',opens_with_salvage:'Se abre con recuperación',unsupported:'No compatible'},
  'it-IT': {opens_normally:'Si apre normalmente',needs_review:'Richiede verifica',opens_with_salvage:'Si apre in recupero',unsupported:'Non supportato'},
  'de-DE': {opens_normally:'Öffnet normal',needs_review:'Überprüfung nötig',opens_with_salvage:'Öffnet mit Wiederherstellung',unsupported:'Nicht unterstützt'},
  'ru-RU': {opens_normally:'Открывается',needs_review:'Нужна проверка',opens_with_salvage:'Открывается с восстановлением',unsupported:'Не поддерживается'},
};
const canonicalCopy: Record<SupportedLocale, { pages:string; limitations:string; idml:string; odg:string; unverified:string; next:string; nextSteps:Record<CanonicalState,string> }> = {
  'en-US': {pages:'Pages',limitations:'Known limitations',idml:'Editable IDML',odg:'Editable ODG',unverified:'Not verified',next:'Recommended next step',nextSteps:{
    opens_normally:'Review a migration preview before converting.',needs_review:'Review the preview and limitations before migration.',
    opens_with_salvage:'Request a recovery review; normal page layout is not proven.',unsupported:'Try manual review or choose a different PUB.'}},
  'en-GB': {pages:'Pages',limitations:'Known limitations',idml:'Editable IDML',odg:'Editable ODG',unverified:'Not verified',next:'Recommended next step',nextSteps:{
    opens_normally:'Review a migration preview before converting.',needs_review:'Review the preview and limitations before migration.',
    opens_with_salvage:'Request a recovery review; normal page layout is not proven.',unsupported:'Try manual review or choose a different PUB.'}},
  'fr-FR': {pages:'Pages',limitations:'Limites connues',idml:'IDML modifiable',odg:'ODG modifiable',unverified:'Non vérifié',next:'Étape suivante',nextSteps:{
    opens_normally:'Examinez un aperçu avant la migration.',needs_review:'Vérifiez l’aperçu et ses limites avant de migrer.',
    opens_with_salvage:'Demandez une vérification des données récupérées.',unsupported:'Demandez un examen manuel ou choisissez un autre PUB.'}},
  'es-ES': {pages:'Páginas',limitations:'Limitaciones conocidas',idml:'IDML editable',odg:'ODG editable',unverified:'Sin verificar',next:'Siguiente paso',nextSteps:{
    opens_normally:'Revise una vista previa antes de migrar.',needs_review:'Revise la vista previa y sus limitaciones.',
    opens_with_salvage:'Solicite una revisión de recuperación.',unsupported:'Solicite revisión manual o elija otro PUB.'}},
  'it-IT': {pages:'Pagine',limitations:'Limitazioni note',idml:'IDML modificabile',odg:'ODG modificabile',unverified:'Non verificato',next:'Prossimo passo',nextSteps:{
    opens_normally:'Controlla l’anteprima prima della migrazione.',needs_review:'Controlla anteprima e limitazioni.',
    opens_with_salvage:'Richiedi una verifica del recupero.',unsupported:'Richiedi una verifica manuale o scegli un altro PUB.'}},
  'de-DE': {pages:'Seiten',limitations:'Bekannte Einschränkungen',idml:'IDML bearbeitbar',odg:'ODG bearbeitbar',unverified:'Nicht geprüft',next:'Nächster Schritt',nextSteps:{
    opens_normally:'Vorschau vor der Migration prüfen.',needs_review:'Vorschau und Einschränkungen überprüfen.',
    opens_with_salvage:'Wiederherstellung prüfen lassen.',unsupported:'Manuelle Prüfung oder andere PUB-Datei versuchen.'}},
  'ru-RU': {pages:'Страницы',limitations:'Известные ограничения',idml:'Редактируемый IDML',odg:'Редактируемый ODG',unverified:'Не подтверждено',next:'Следующий шаг',nextSteps:{
    opens_normally:'Проверьте предварительный просмотр перед переносом.',needs_review:'Проверьте внешний вид и ограничения перед переносом.',
    opens_with_salvage:'Запросите проверку восстановленного содержимого. Геометрия страниц не подтверждена.',
    unsupported:'Потребуется ручная проверка или другой PUB.'}},
};

function setCookie(name:string,value:string){document.cookie=`${name}=${encodeURIComponent(value)}; Max-Age=31536000; Path=/; SameSite=Lax`;}
function sizeLabel(value:number, locale:SupportedLocale){
  const mb=value>=1024*1024;
  const amount=mb?value/1024/1024:value/1024;
  return `${new Intl.NumberFormat(locale,{maximumFractionDigits:1}).format(amount)} ${mb?'MB':'KB'}`;
}

export default function LandingClient({initialCountry,initialLocale}:{initialCountry:SupportedCountry;initialLocale:SupportedLocale}) {
  const inputRef=useRef<HTMLInputElement>(null);
  const [country,setCountry]=useState(initialCountry);
  const [locale,setLocale]=useState(initialLocale);
  const [file,setFile]=useState<File|null>(null);
  const [email,setEmail]=useState('');
  const [consent,setConsent]=useState(false);
  const [dragging,setDragging]=useState(false);
  const [stage,setStage]=useState<'idle'|'uploading'|'queued'>('idle');
  const [progress,setProgress]=useState(0);
  const [check,setCheck]=useState<{id:string;token:string}|null>(null);
  const [status,setStatus]=useState<PublicStatus|null>(null);
  const [error,setError]=useState('');
  const copy=COPY[locale];
  const validEmail=useMemo(()=>/^\S+@\S+\.\S+$/.test(email.trim()),[email]);

  useEffect(()=>{document.documentElement.lang=locale;},[locale]);

  function changeCountry(next:SupportedCountry){setCountry(next);setCookie('chaptera-country',next);}
  function changeLocale(next:SupportedLocale){setLocale(next);setCookie('chaptera-locale',next);}
  function choose(next:File|null){
    setError('');setStatus(null);setCheck(null);setProgress(0);
    if(!next)return setFile(null);
    if(!next.name.toLowerCase().endsWith('.pub')){setFile(null);return setError(copy.errors.pubOnly);}
    if(next.size===0||next.size>MAX_BYTES){setFile(null);return setError(copy.errors.fileSize);}
    setFile(next);
  }
  async function submit(){
    if(!file||!validEmail||!consent||stage!=='idle')return;
    setError('');setStage('uploading');
    try{
      const safeName=file.name.replace(/[^a-zA-Z0-9._-]+/g,'_').slice(-120);
      const blob=await upload(`incoming/${safeName}`,file,{
        access:'private',handleUploadUrl:'/api/upload',contentType:file.type||'application/octet-stream',
        multipart:file.size>8*1024*1024,onUploadProgress:({percentage})=>setProgress(Math.round(percentage)),
      });
      const response=await fetch('/api/checks',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({
        email:email.trim(),blobUrl:blob.url,pathname:blob.pathname,filename:file.name,byteLength:file.size,locale,country,
      })});
      const body=await response.json();
      if(!response.ok)throw new Error(body.error||copy.errors.start);
      setCheck({id:body.id,token:body.token});setStage('queued');setStatus({status:body.status,emailStatus:'pending'});
    }catch(cause){setStage('idle');setError(cause instanceof Error?cause.message:copy.errors.upload);}
  }

  useEffect(()=>{
    if(!check||status?.status==='complete'||status?.status==='failed')return;
    let alive=true;
    const poll=async()=>{try{const response=await fetch(`/api/checks/${encodeURIComponent(check.id)}?token=${encodeURIComponent(check.token)}`,{cache:'no-store'});const body=await response.json();if(alive&&response.ok)setStatus(body);}catch{}};
    void poll();const timer=window.setInterval(poll,3500);return()=>{alive=false;window.clearInterval(timer);};
  },[check,status?.status]);

  const busy=stage!=='idle';const complete=status?.status==='complete';
  const canonical=complete&&status?.result&&'canonical' in status.result?status.result.canonical:null;
  const legacy=complete&&status?.result&&!('canonical' in status.result)?status.result:null;
  return <main data-country={country} data-locale={locale}><div className="shell">
    <nav className="nav">
      <div className="brandWrap"><div className="brand">Chaptera</div><div className="regionHint">{copy.detected}: {country}</div></div>
      <div className="localeControls">
        <label><span>{copy.region}</span><select value={country} onChange={e=>changeCountry(e.target.value as SupportedCountry)}>{COUNTRY_OPTIONS.map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></label>
        <label><span>{copy.language}</span><select value={locale} onChange={e=>changeLocale(e.target.value as SupportedLocale)}>{LOCALE_OPTIONS.map(([v,l])=><option key={v} value={v}>{l}</option>)}</select></label>
      </div>
    </nav>
    <section className="hero">
      <div className="eyebrow">{copy.eyebrow}</div><h1>{copy.title}</h1><p>{copy.body}</p>
      <div className="marketStrip"><span className="marketDot"/><span>{copy.badge}</span></div>
      <div className="card">
        {!check?<>
          <div className={`drop ${dragging?'active':''}`} onClick={()=>!busy&&inputRef.current?.click()}
            onDragOver={e=>{e.preventDefault();if(!busy)setDragging(true)}} onDragLeave={()=>setDragging(false)}
            onDrop={e=>{e.preventDefault();setDragging(false);if(!busy)choose(e.dataTransfer.files?.[0]??null)}}>
            <input ref={inputRef} type="file" accept=".pub,application/vnd.ms-publisher,application/x-mspublisher" onChange={e=>choose(e.target.files?.[0]??null)}/>
            <div><div className="dropIcon">↥</div><strong>{copy.drop}</strong><small>{copy.dropHint}</small></div>
          </div>
          {file&&<div className="filePill"><div><b>{file.name}</b><span>{sizeLabel(file.size,locale)}</span></div>{!busy&&<button type="button" onClick={()=>choose(null)}>{copy.remove}</button>}</div>}
          <div className="formRow"><input className="email" type="email" placeholder={copy.email} value={email} disabled={busy} onChange={e=>setEmail(e.target.value)}/>
            <button className="cta" type="button" disabled={!file||!validEmail||!consent||busy} onClick={submit}>{busy?copy.uploading:copy.submit}</button></div>
          <label className="consent"><input type="checkbox" checked={consent} disabled={busy} onChange={e=>setConsent(e.target.checked)}/><span>{copy.consent}</span></label>
          {stage==='uploading'&&<div className="progressWrap"><div className="progressBar"><div style={{width:`${progress}%`}}/></div><div className="progressText"><span>{copy.progress}</span><span>{progress}%</span></div></div>}
          {error&&<div className="error">{error}</div>}
        </>:<div className="status"><div className="statusTop"><div><h3>{complete?copy.ready:status?.status==='failed'?copy.failed:status?.status==='processing'?copy.processing:copy.queued}</h3>
          <p>{complete?(canonical?canonicalLabels[locale][canonical.state]:legacy?.summary||copy.ready):copy.waiting}</p></div><div className="statusChip">{status?statusLabels[locale][status.status]:statusLabels[locale].queued}</div></div>
          {canonical&&<>
            <div className="resultGrid">
              <div className="metric"><span>{copy.compatibility}</span><b>{canonicalLabels[locale][canonical.state]}</b></div>
              {canonical.contentSummary?.page_count!==undefined&&<div className="metric"><span>{canonicalCopy[locale].pages}</span><b>{canonical.contentSummary.page_count}</b></div>}
              <div className="metric"><span>{canonicalCopy[locale].idml}</span><b>{canonicalCopy[locale].unverified}</b></div>
              <div className="metric"><span>{canonicalCopy[locale].odg}</span><b>{canonicalCopy[locale].unverified}</b></div>
              <div className="metric"><span>{copy.emailMetric}</span><b>{status?.emailStatus==='sent'?copy.emailSent:copy.emailPrepared}</b></div>
            </div>
            {canonical.limitations.length>0&&<div className="resultDetails"><strong>{canonicalCopy[locale].limitations}</strong><ul>{canonical.limitations.map(l=><li key={l.code}>{l.message}</li>)}</ul></div>}
            <div className="resultDetails"><strong>{canonicalCopy[locale].next}</strong><p>{canonicalCopy[locale].nextSteps[canonical.state]}</p></div>
          </>}
          {legacy&&<div className="resultGrid"><div className="metric"><span>{copy.compatibility}</span><b>{compatibilityLabels[locale][legacy.compatibility]}</b></div><div className="metric"><span>{copy.publisherFamily}</span><b>{legacy.publisherFamily||copy.notIdentified}</b></div><div className="metric"><span>{copy.emailMetric}</span><b>{status?.emailStatus==='sent'?copy.emailSent:copy.emailPrepared}</b></div></div>}
          {status?.status==='failed'&&<div className="error">{copy.reliableFailure}</div>}</div>}
      </div>
    </section>
    <section className="featureGrid"><div className="feature"><b>{copy.privateTitle}</b><p>{copy.privateBody}</p></div><div className="feature"><b>{copy.evidenceTitle}</b><p>{copy.evidenceBody}</p></div><div className="feature"><b>{copy.emailTitle}</b><p>{copy.emailBody}</p></div></section>
    <footer className="footer"><span>{copy.footerLeft}</span><span>{copy.footerRight}</span></footer>
  </div></main>;
}
