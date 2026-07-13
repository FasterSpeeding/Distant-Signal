export interface ValidityPeriod {
  fromDate: string;
  toDate: string | null;
  isNow: boolean;
}

export interface AffectedRoute {
  from: string;
  to: string;
}

export interface Disruption {
  category: string;
  description: string;
  affectedStops: string[];
  affectedRoutes: AffectedRoute[];
  source: string | null;
}

export interface SampleStats {
  total: number;
  delayed: number;
  cancelled: number;
  avgDelayMinutes: number;
}

export interface LineStatus {
  statusSeverity: number;
  statusSeverityDescription: string;
  reason: string;
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned';
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;
  sampleStats?: SampleStats;
}

export interface LineStatusReport {
  $type: string;
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
}

export interface LineStatusHistoryEntry extends LineStatusReport {
  computedAt: string;
}

export interface Preferences {
  pinnedLines: string[];
  pinnedStations: string[];
}

export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom';
}

export interface CustomLineDetail {
  id: string;
  name: string;
  operators: string[];
  stations: string[];
  headcodePrefixes: string[];
  destinationCrsFilter: string[];
}

export interface LineDefinitionSummary {
  stations: string[];
  operators: string[];
}
