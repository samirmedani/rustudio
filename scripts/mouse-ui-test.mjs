// Contrat utilisateur : modifier localement, puis une seule requête explicite d'enregistrement.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const html=readFileSync(new URL('../ui/index.html',import.meta.url),'utf8');
const code=html.slice(html.indexOf('var mouseUI = null'),html.indexOf('function renderMouse()'));
function harness(){
  const calls=[],messages=[];
  const c={paintMouse(){},loadMouse(){},toast:s=>messages.push(s),postModule:async (...args)=>{calls.push(args);return null;}};
  runInNewContext(code,c);
  c.mouseState={profile:1,wired:false,dpi_max:42000,stages:[{x:800,y:800},{x:1600,y:1600}],active_stage:1,polling_hz:1000,debounce_ms:3,motion_sync:false};
  return {c,calls,messages};
}
{
  const {c,calls}=harness();
  c.mouseEdit('polling_hz',2000);c.mouseEdit('motion_sync',true);c.mouseEdit('dpi-0-x','850');
  assert.equal(calls.length,0,'aucune écriture pendant les modifications');
  c.mouseState={...c.mouseState,debounce_ms:4};
  assert.equal(c.mouseValue('polling_hz'),2000,'un rafraîchissement garde le brouillon');
  assert.equal(c.mouseValue('debounce_ms'),4,'un champ non modifié suit la lecture réelle');
  await c.mouseSave();
  assert.equal(calls.length,1);assert.equal(calls[0][1],'settings');
  assert.equal(calls[0][2].changes.stages[0].y,800);
  assert.equal(calls[0][2].changes.stages[0].x,850);
  assert.ok(!('debounce_ms' in calls[0][2].changes),'les réglages non modifiés ne sont pas écrits');
  assert.equal(c.mouseValue('polling_hz'),2000,'une erreur conserve les changements');
  c.mouseDiscard();assert.equal(Object.keys(c.mouseDraft).length,0);assert.equal(calls.length,1,'annuler ne touche pas au matériel');
}
{
  const {c,calls}=harness();c.mouseEdit('dpi-0-x','55');await c.mouseSave();assert.equal(calls.length,0);
  c.mouseEdit('dpi-0-x','');await c.mouseSave();assert.equal(calls.length,0);
  c.mouseEdit('dpi-0-x','850');c.mouseState.profile=2;await c.mouseSave();assert.equal(calls.length,0,'pas d’écriture sur un autre profil');
  assert.equal(c.mouseDraftProfile,1);
  c.mouseState=null;await c.mouseSave();assert.equal(calls.length,0);
  c.mouseDiscard();assert.equal(c.mouseValue('dpi-0-x'),800,'annuler hors ligne retrouve la dernière lecture, sans écrire');
}
{
  const {c,calls}=harness();c.mouseEdit('motion_sync',true);c.mouseEdit('motion_sync',false);
  assert.equal(Object.keys(c.mouseDraft).length,0,'revenir à la valeur enregistrée annule le changement');
  let finish;c.postModule=(...args)=>{calls.push(args);return new Promise(resolve=>{finish=resolve;});};
  c.mouseEdit('polling_hz',2000);const saved=c.mouseSave();c.mouseSave();c.mouseEdit('polling_hz',4000);
  assert.equal(calls.length,1,'pas de double enregistrement');
  finish({ok:true,state:{...c.mouseState,polling_hz:2000}});await saved;
  assert.equal(c.mouseState.polling_hz,2000);assert.equal(Object.keys(c.mouseDraft).length,0);
}
console.log('Souris UI : brouillon sans écriture, enregistrement groupé, annulation, rafraîchissement, erreur et profil vérifiés.');
