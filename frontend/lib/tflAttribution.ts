/** TfL's attribution, in the wording its transport data terms require
 * (modified Open Government Licence v2.0). Its own module so a client
 * component (the untracked-leg stop picker) and the server-rendered
 * footer and `/attribution` page share one copy. Do not paraphrase. */

/** Wherever TfL open data is presented. */
export const TFL_CREDIT = 'Powered by TfL Open Data';

/** With data derived from Ordnance Survey and Geomni mapping, which TfL's
 * StopPoint coordinates are (the stop lookup, docs/superpowers/specs/
 * 2026-10-20-tfl-stop-lookup-design.md). Verbatim apart from the bracketed
 * year of the Geomni line. */
export const TFL_OS_STATEMENT = 'Contains OS data © Crown copyright and database rights 2016';
export const TFL_GEOMNI_STATEMENT = 'Geomni UK Map data © and database rights 2019';

/** The credit and both mapping statements as one line, for where TfL stop
 * data is shown. */
export const TFL_STOPS_ATTRIBUTION = `${TFL_CREDIT}. ${TFL_OS_STATEMENT}. ${TFL_GEOMNI_STATEMENT}.`;
