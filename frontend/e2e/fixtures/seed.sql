
-- ===========================================================================
-- Fixture seed data for the `frontend-e2e` CI job (.github/workflows/ci.yml)
-- and for the local dev screenshot-sweep job it was originally written for
-- (`.devdata/gen_seed.py`, not itself tracked in this repo). Every row here
-- backs one of the `E2E_*` env vars the specs in frontend/e2e/ read -- see
-- accessibility.spec.ts's own "Fixtures" comment for the full rationale on
-- each var.
--
-- Applied via `psql -v ON_ERROR_STOP=1 "$DATABASE_URL" -f frontend/e2e/fixtures/seed.sql` after
-- `sqlx migrate run` and before the `api` binary starts.
--
-- Idempotent (ON CONFLICT DO NOTHING/DO UPDATE throughout) so re-running it
-- against an already-seeded database is safe, EXCEPT for the two
-- `train_subscriptions` rows (see the unresolved one's own comment below for
-- why their insert order matters and why that one is not idempotent).
--
-- Dates: `2026-09-17` is hardcoded into `trains.service_date` and a few
-- `validity`/`validity_periods` JSONB payloads below. Checked against
-- `crates/api`'s actual staleness logic (`journey::may_have_arrived`,
-- crates/api/src/data/journey.rs) before committing to a fixed date rather
-- than a computed one: a service_date/ETA that far in the past relative to
-- real wall-clock `NOW()` makes `may_have_arrived` return `true`, which only
-- changes the train's rendered copy to past tense (commit 7f7beaf4, "stale
-- ETA under 'may have arrived' now reads past-tense") -- it does not 404,
-- error, or drop any content the accessibility/layout specs assert on.
-- `get_by_uid_and_date` (crates/api/src/routes/train.rs) also has no
-- age-based rejection of its own; it reads whatever the `trains` row says
-- regardless of how old `service_date` is. Kept as a fixed date (less
-- invasive than templating every JSONB blob with a computed one) on the
-- judgement that this suite only checks rendering/accessibility, never "is
-- this today".
-- ===========================================================================

-- ---------------------------------------------------------------------
-- Users
-- ---------------------------------------------------------------------
-- Neither user carries any group: `/chat` mounts `ChatPanel` for every
-- logged-in user (no access group since 2026-10-14), which chat.spec.ts's
-- logged-in cases rely on.
INSERT INTO users (id, name, username, groups, created_at, last_login_at)
VALUES
  ('fixture-owner', 'Ada Fixture', 'ada-fixture', '{}', NOW(), NOW()),
  ('fixture-member', 'Rider Fixture', 'rider-fixture', '{}', NOW(), NOW())
ON CONFLICT (id) DO NOTHING;

-- Session for fixture-owner. Cookie value is the RAW token (E2E_SESSION_COOKIE,
-- set on the browser via Playwright storageState); this row stores its
-- SHA-256 hex digest, per auth::hash_session_token.
INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at)
VALUES ('e28c7c24cdbfa7cfa61270be2478ba0fd1ff69251db2a127c1777769c6a8c973', 'fixture-owner', NULL, NOW(), NOW() + INTERVAL '14 days')
ON CONFLICT (id) DO NOTHING;

-- ---------------------------------------------------------------------
-- An UNRESOLVED train subscription for E2E_TRACKING_ID.
--
-- accessibility.spec.ts's `/train/by-id/[trackingId]` test needs a
-- subscription whose `resolution_status` is NOT 'resolved': a resolved one
-- `redirect()`s to `/train/[uid]/[date]`
-- (frontend/app/train/by-id/[trackingId]/page.tsx), which would make that
-- test silently audit the wrong route instead (it asserts the URL didn't
-- change specifically to catch this).
--
-- This MUST be the first row inserted into train_subscriptions (before the
-- resolved one further down) so it lands on a fresh database with
-- `id = 1`, matching E2E_TRACKING_ID=1 -- there is no other unique key on
-- this table to upsert against by value, so insert-order-on-a-fresh-db is
-- the only lever available. Safe for CI's always-fresh database; re-running
-- this script against an already-seeded one will insert a SECOND unresolved
-- row with a new id rather than updating this one, which is fine for local
-- iteration but is the one non-idempotent step in this file.
INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, pin_operator, resolution_status, tracked_at, notifications_enabled)
VALUES ('fixture-owner', '2026-09-17', 'EUS', '2026-09-17T08:15:00Z', 'MAN', 'AW', 'pending', NOW(), TRUE);

-- ---------------------------------------------------------------------
-- Custom line (owned by fixture-owner)
-- ---------------------------------------------------------------------
INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, user_id, created_at)
VALUES ('custom-fixture-commute', 'My Fixture Commute', '{GR}', '{KGX,EUS}', '{}', '{}', 'fixture-owner', NOW())
ON CONFLICT (id) DO NOTHING;

-- ---------------------------------------------------------------------
-- Line status + history (catalogue lines: lner-ecml, c2c)
-- ---------------------------------------------------------------------
INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at, source)
VALUES
  ('lner-ecml', 'LNER East Coast Main Line', 'national-rail', '{GR}',
   '[{"severity":9,"reason":"Fixture: signalling problems near Peterborough","validity":{"from_date":"2026-09-17T06:00:00Z","to_date":null,"is_now":true}}]'::jsonb,
   NOW(), 'aggregator'),
  ('c2c', 'c2c (London, Tilbury & Southend line)', 'national-rail', '{CC}',
   '[{"severity":10,"reason":"Good Service","validity":{"from_date":"2026-09-17T00:00:00Z","to_date":null,"is_now":true}}]'::jsonb,
   NOW(), 'aggregator')
ON CONFLICT (line_id) DO UPDATE SET
  statuses = EXCLUDED.statuses, computed_at = EXCLUDED.computed_at;

INSERT INTO line_status_history (line_id, statuses, computed_at)
VALUES
  ('lner-ecml', '[{"severity":9,"reason":"Fixture: signalling problems near Peterborough","validity":{"from_date":"2026-09-17T06:00:00Z","to_date":null,"is_now":true}}]'::jsonb, NOW() - INTERVAL '2 hours'),
  ('lner-ecml', '[{"severity":10,"reason":"Good Service","validity":{"from_date":"2026-09-16T00:00:00Z","to_date":"2026-09-17T06:00:00Z","is_now":false}}]'::jsonb, NOW() - INTERVAL '1 day'),
  -- Past statuses in the three severity groups no current status shows
  -- (severe, planned, informational), so the history page renders a
  -- `StatusBadge` in all five and accessibility.spec.ts's axe sweep checks
  -- every severity pill's contrast in both schemes. History only: the
  -- line's current status, and so every other page, is unchanged.
  ('lner-ecml', '[{"severity":6,"reason":"Fixture: a points failure at Grantham","validity":{"from_date":"2026-09-15T16:00:00Z","to_date":"2026-09-15T19:00:00Z","is_now":false}}]'::jsonb, NOW() - INTERVAL '1 day 3 hours'),
  ('lner-ecml', '[{"severity":4,"reason":"Fixture: engineering work between Peterborough and Grantham","validity":{"from_date":"2026-09-15T00:00:00Z","to_date":"2026-09-15T05:00:00Z","is_now":false}}]'::jsonb, NOW() - INTERVAL '1 day 6 hours'),
  ('lner-ecml', '[{"severity":0,"reason":"Fixture: a special timetable for an event at York","validity":{"from_date":"2026-09-14T18:00:00Z","to_date":"2026-09-14T23:00:00Z","is_now":false}}]'::jsonb, NOW() - INTERVAL '1 day 9 hours');

-- ---------------------------------------------------------------------
-- One TOC, so /operators has a card to search (list-filters.spec.ts)
-- ---------------------------------------------------------------------
-- c2c only: an operator is listed for a `tocs` row with a status row
-- (the c2c line above). Not GR, which the screenshotted line, station,
-- train and incident pages name: a GR row would rename it on most of the
-- visual baselines. No screenshotted page names CC.
INSERT INTO tocs (atoc_code, name, legal_name, atoc_member, station_operator)
VALUES ('CC', 'c2c', 'c2c Limited', TRUE, TRUE)
ON CONFLICT (atoc_code) DO NOTHING;

-- ---------------------------------------------------------------------
-- An empty train population for c2c, so its line page draws "Trains on
-- this line" with its links (list-filters.spec.ts). Yesterday to
-- tomorrow, London time, as the page asks for London's today. Not
-- lner-ecml, whose line page is a visual baseline.
-- ---------------------------------------------------------------------
INSERT INTO schedule_line_population (line_id, service_date, population)
SELECT 'c2c', (NOW() AT TIME ZONE 'Europe/London')::date + offset_days, '[]'::jsonb
FROM generate_series(-1, 1) AS offset_days
ON CONFLICT (line_id, service_date) DO NOTHING;

-- ---------------------------------------------------------------------
-- Stations with real accessibility payloads (captured production fixtures)
-- ---------------------------------------------------------------------
INSERT INTO stations (crs, name, latitude, longitude, station_operator, accessibility, fetched_at)
VALUES
  ('KGX', 'London Kings Cross', 51.5308, -0.1238, 'GR', $kgxacc${"carParks":{"accessibleParkingSpacesAvailable":true,"carParks":[{"accessibleLocations":[{"accessibilityInfo":{"accessibleParkingSpaces":"There is/are 3 accessible bay(s) available at this car parking location","evChargingAccessible":"There are No EV Charging points available in the accessible bays","evChargingNonAccessible":"There are no EV charging points in standard parking bays","helpPointClose":"There is a Help Point close to the accessible parking","location":"There is accessible parking outside the station","numberOfAccessibleParkingSpaces":3,"parkingDuration":"Parking is short stay","rampDeployable":"Ramps can be deployed from the rear and side of a vehicle","sizeOfOffRoadBays":"The car park bays measure 2.4 x 4.8m & 1.2m zone front & at least 1 side","sizeOfRoadBays":"There are no roadside bays"},"name":"near Taxi Drop- Off Pancras Road"}],"accessibleSpacesNotes":null,"cctv":null,"charges":null,"freeParking":null,"name":"Accessible Parking","notes":null,"numberOfAccessibleSpaces":3,"numberOfSpaces":null,"openingHours":null,"operator":null},{"accessibleLocations":null,"accessibleSpacesNotes":null,"cctv":false,"charges":{"annualRate":null,"dailyRate":null,"monthlyRate":null,"offPeakRate":null,"perHourRate":null,"saturdayRate":null,"sixMonthlyRate":null,"sundayRate":null,"threeMonthlyRate":null,"weeklyRate":null},"freeParking":false,"name":"Station Car Park","notes":null,"numberOfAccessibleSpaces":null,"numberOfSpaces":322,"openingHours":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"01:11:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"}],"operator":{"contactDetails":{"emailAddress":"info@cp-plus.co.uk","name":"London Kings Cross Car Park 1 Contact Details","note":null,"operatorName":"CP Plus","postalAddress":{"addressLine1":"-","addressLine2":"-","addressLine3":null,"addressLine4":null,"addressLine5":null,"postcode":null},"primaryTelephoneNumber":"0207 431 4001","url":"https://www.nationalrail.co.uk/"}}}],"numberOfAccessibleSpaces":3,"numberOfSpaces":322,"parkingSpacesAvailable":true},"cycling":{"cctv":true,"cycleHireNotes":null,"cycleStorageAvailable":true,"location":"<p>Platforms 1, 8 &amp; 9</p>","sheltered":true,"spaces":{"notes":null,"numberOfSpaces":204},"typesOfStorage":["Stands"]},"dropOffPickUp":{"available":true,"location":"<p>There are 3 accessible parking spaces with 1 hour parking outside Kings Cross station on Pancras Road for Blue Badge Holders. </p>","notes":"<p>There is a help point here that can be used to request Passenger Assistance from these spaces.</p>","points":[{"carAccommodationCount":"There is space for up to 5 vehicles to drop-off and/or pick-up","easyToDeployRamp":null,"helpPointClose":"There is no Help Point close to the drop-off and/or pick-up point","name":"Taxi Drop-off Point, Pancras Road (Island area)","sameSideAsStation":"The drop-off and/or pick-up point is accessed via an uncontrolled road crossing such as a zebra crossing","typeOfFacility":null,"waitingAreaForDisabledPickup":"This is a drop-off point only"},{"carAccommodationCount":"There is space for up to 5 vehicles to drop-off and/or pick-up","easyToDeployRamp":null,"helpPointClose":"There is no Help Point close to the drop-off and/or pick-up point","name":"Drop-off Point, Pancras Road (Station Side)","sameSideAsStation":"The drop-off and/or pick-up point is on the same side of the road as the station","typeOfFacility":null,"waitingAreaForDisabledPickup":"This is a drop-off point only"}]},"helpAndSupport":{"accessibilityInformation":"There is information about accessibility","announcements":"Announcements are made both visually and audibly","customerInformationScreens":["DepartureScreens","Announcements","ArrivalScreens"],"helpPoints":{"available":true,"inductionLoop":"Induction loops are only in Help Points","location":"Help Points are available inside and outside this station","notes":"<p>Customer help points are available at the platforms and at the taxi rank as well as using the rail information point at the centre of the main concourse.</p>"},"inductionLoops":"Induction loops are only in Help Points","informationPoints":"There are information points","printedLocalInformation":"There is printed information available","staffHelp":{"available":true,"location":"<p>.</p>","notes":"$34","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"00:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:30:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":null},"staffInformation":["Yes - from information point","Yes - from help point","Yes - from ticket office"]},"lifts":{"available":true,"liftsInfo":[{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift 6, Footbridge to Platforms 6 & 7 Mobility Impaired Person","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift 7, Footbridge to Platform 8 Mobility Impaired Person","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift which has a 1.5 metre turning circle inside","name":"Lift 1, Western Concourse (Behind Pret a Manger) Mobility Impaired Person","openingWidth":"Lift has clear door widths 1.1m wide","operationalAnnouncements":"There are audio announcements provided inside this lift only"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift which has a 1.5 metre turning circle inside","name":"Lift 2, Western Concourse behind Starbucks Mobility Impaired Person","openingWidth":"Lift has clear door widths greater than 1.1m wide","operationalAnnouncements":"There are audio announcements provided inside this lift only"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift which has a 1.5 metre turning circle inside","name":"Lift M, Concourse to First Class Lounge","openingWidth":"Lift has clear door widths 1.1m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls may not be accessible to some people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift for Fullers Pub only, Lift T, Accessible","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are no audio announcements or they are not audible in this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift 3, Footbridge to Platforms 0 & 1 Mobility Impaired Person","openingWidth":"Lift has clear door widths 1.1m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift 4, Footbridge to Platforms 2 & 3 Mobility Impaired Person","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift is a through lift with no turning circle","name":"Lift 5, Footbridge to Platforms 4 & 5 Mobility Impaired Person","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"}],"statement":"There are lifts"},"loungesAndWaiting":{"faithRoom":"There is no multi-faith room at this station","firstClass":{"available":true,"location":"<p><strong>Location:</strong> Next to Waitrose</p>","notes":"$35","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"21:25:00.000","startTime":"07:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"20:15:00.000","startTime":"08:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"20:15:00.000","startTime":"09:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":null},"firstClassLounges":[{"capableOfBeingHeated":"The First class lounge is capable of being heated and cooled","helpPointClose":"There is no Help Point close to the First Class lounge","inSightOfCis":"There is seating with a view of customer information screens but not trains in the First Class lounge","minimumSeatingProvision":"There is only bench seating","name":"First Class lounge(LNER), off of the South Side Concourse"}],"quietRoom":"There is no quiet room at this station","seatingArea":{"available":false,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"shelteredWaitingAvailable":true,"waitingFacility":{"available":true,"location":"<p>All platforms are partially or fully covered with a canopy.</p>","notes":null,"openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"23:30:00.000","startTime":"06:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"22:30:00.000","startTime":"06:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"23:30:00.000","startTime":"07:45:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":null},"waitingRooms":[{"automatedDoors":null,"heatingOrCoolingAvailable":null,"helpPointClose":null,"inSightOfCis":null,"minimumSeatingStandard":null,"name":"Family Lounge, main concourse"},{"automatedDoors":"The doors to the waiting room are automated","heatingOrCoolingAvailable":"The waiting area is capable of being heated and cooled","helpPointClose":"There is a Help Point close to the waiting room","inSightOfCis":"There is seating within view of the Customer Information Screen but not of trains in the waiting room","minimumSeatingStandard":"There is seating with backrests and armrests on both sides","name":"Elphick Room Mobility Assistance Lounge, off of the Main Concourse"}]},"platformFacilities":{"entranceLevels":"The platforms are level with the Main Entrance of the station","numberOfPlatforms":11,"platforms":[{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 4","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 5","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 6","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 7","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 9","seatingAtIntervals":"There is seating at regular intervals of 50 metres apart or less","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 10","seatingAtIntervals":"Seating is limited on this platform","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 0","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 1","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 2","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 3","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 8","seatingAtIntervals":"There is no seating on this platform","waitingType":null}],"tactileWarnings":"There are tactile warnings on all platforms in use"},"staffAssistance":{"announcements":"Announcements are made both visually and audibly","customerInformation":["DepartureScreens","Announcements","ArrivalScreens"],"customerServiceNotes":"<p>Help is available at the Rail Information Point in the centre of the Main Concourse.</p>","helpPoints":{"available":true,"inductionLoop":"All Help Points have induction loops","location":"Help Points are available inside and outside this station","notes":"<p>Customer help points are available at the platforms and at the taxi rank as well as using the rail information point at the centre of the main concourse.</p>"},"helpline":{"available":true,"location":null,"notes":"<p>0800 022 3720</p><p></p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":{"emailAddress":null,"name":"London Kings Cross Help Line Contact Details","note":"<p>If you wish to book assistance but are not sure which train operator you are travelling with, you can call 0800 022 3720. On calling, you will be referred to the appropriate train operator.</p>","operatorName":null,"primaryTelephoneNumber":"0800 022 3720"}},"informationAvailableFromStaff":["Yes - from information point","Yes - from help point","Yes - from ticket office"],"staffHelp":{"available":true,"location":"<p>.</p>","notes":"$36","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"00:36:00.000","startTime":"05:00:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"01:36:00.000","startTime":"05:30:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":null}},"stationAccessibility":{"escalatorInformation":null,"inductionLoop":{"provision":"Induction loops are only in Help Points","ticketCounters":null},"nearestAccessibleStations":null,"passengerAssistance":[{"available":true,"location":"<p>Customer information Point </p>","name":"London Kings Cross Passenger Assistance Meeting Point 0","notes":"<p>Customer information desk Located on the main concourse, left of the travel centre. Please check the opening times on National Rail Enquiries website</p>"},{"available":true,"location":"<p>First Class Lounge </p>","name":"London Kings Cross Passenger Assistance Meeting Point 1","notes":"<p>The first class lounge is located on the mezzanine level, accessed by lift under the departure boards or via an escalator near the information point.</p>"},{"available":true,"location":"<p>Assistance Meeting Point </p>","name":"London Kings Cross Passenger Assistance Meeting Point 2","notes":"<p>When the customer information point is closed, go to the contractors reception next to Leon food outlet located under the train information boards.</p>"}],"stepFreeCategory":{"category":"A, Compliant step-free access to all platform(s)","levelAccess":null,"notes":"<p>This is a Category A station. This station has step-free level access to all platforms.</p><p>Lifts also provided between concourse up to the Mezzanine and down to the London Underground ticket hall.</p><p>The Assistance Meeting Point varies - please check with your operator.</p>"},"tactilePaving":"There are tactile warnings on all platforms in use","ticketBarriers":{"available":true,"location":null,"names":["Ticket barriers, Station Concourse","Ticket barriers, from Platforms 6, 7 & 8 (Exit)","Ticket barriers, Platforms 0-8"],"notes":"<p>Yes there are ticket gates to all platforms. All platforms can be accessed by wide ticket gates.</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"trainRamp":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null,"storage":"Ramps are kept at this station to provide assistance"},"wheelchairsAvailable":true},"stationFacilities":{"atm":{"available":true,"location":"<p>Located on the main concourse</p>","notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"cctvAvailable":true,"currencyExchange":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"defibrillator":null,"limitedService":null,"lostProperty":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"17:30:00.000","startTime":"09:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"London Kings Cross Lost Property Contact Details","note":null,"operatorName":"Excess Baggage Company","primaryTelephoneNumber":"0330 024 0215"}},"luggageStorage":{"available":true,"location":"<p> </p>","notes":null,"openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"23:00:00.000","startTime":"07:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"London Kings Cross Luggage Storage Contact Details","note":null,"operatorName":"Excess Baggage Company","primaryTelephoneNumber":"020 3468 4690"}},"payPhones":false,"postBox":null,"refreshments":{"available":true,"location":"<p>Station concourse</p>","notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"requestStop":null,"shops":{"available":true,"location":null,"notes":"<p>Cafes, bars, kiosks, restaurants, delicatessens, confectioners, groceries,&#160; food and drink to eat in or take away. clothes, stationery, flowers, photo processing, mobile phones, bureau de change, shoe repairs, key cutting, Pharmacy and prescriptions, optician, fitness, health and beauty services.</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"trolleys":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"wifi":{"available":true,"location":null,"notes":"<p><a href=\"http://www.btwifi.co.uk/find/\" title=\"\">Find WiFi Hotspots around London Kings Cross station</a></p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null}},"toiletsAndChanging":{"showers":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"toilets":{"accessibleToiletsAvailable":true,"available":true,"babyChangingAvailable":true,"changingPlacesToiletsAvailable":true,"location":"<p>The toilets including accessible toilets are located next to platform 9. There also toilets on the Mezzanine Level.</p>","locations":[{"accessibleHandles":null,"name":"Male toilet, Mezzanine Level","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Female toilet, Mezzanine Level","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Male toilet, (LNER)First Class lounge","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Female toilet, (LNER) First Class Lounge","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Accessible toilet, Mezzanine Level","radarKeyAvailable":null},{"accessibleHandles":"The toilet entry handles may be operated by most people","name":"Accessible toilet, (LNER) First Class Lounge off of South Concourse","radarKeyAvailable":"This toilet is accessed with a RADAR key"},{"accessibleHandles":null,"name":"Baby changing facility, Ground Floor South Concourse","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Female toilet, Ground Floor South Concourse","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Male toilet, Ground Floor South Concourse","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Accessible toilet(1), Ground Floor South Concourse","radarKeyAvailable":"This toilet is not accessed with a RADAR key"},{"accessibleHandles":null,"name":"Accessible toilet(2), Ground Floor South Concourse","radarKeyAvailable":null},{"accessibleHandles":"The toilet entry handles may be operated by most people","name":"Changing Places toilet, off of South Concourse","radarKeyAvailable":"This toilet is not accessed with a RADAR key"},{"accessibleHandles":null,"name":"Baby change facility, Mezzanine Level","radarKeyAvailable":null}],"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null}},"transportLinks":{"airport":{"available":true,"notes":"<p>Connections from LUL available</p>"},"bus":{"available":true,"notes":"<p>There are bus stops in streets surrounding the station on Euston Road, Goods Way and York Way. More details at&#160;<a href=\"https://www.nationalrail.co.uk/posters/KGX.pdf\" title=\"\">https://www.nationalrail.co.uk/posters/KGX.pdf</a></p><p><strong>Accessibility on London Buses</strong></p><p>All Transport for London buses are accessible by step free access&#160;on Transport for London&#39;s website <a href=\"https://www.tfl.gov.uk/transport-accessibility/wheelchair-access-and-avoiding-stairs\" title=\"\">https://tfl.gov.uk/transport-accessibility/wheelchair-access-and-avoiding-stairs</a></p><p>More information about Transport for London buses can be found at&#160;<a href=\"https://tfl.gov.uk/modes/buses/\" title=\"\">https://tfl.gov.uk/modes/buses/</a></p>"},"carHire":{"available":false,"notes":null},"port":{"available":false,"notes":null},"replacementBus":{"available":true,"maps":[],"notes":"<p>Lumo: Drop-off &amp; Pick-up St Pancras Rd, left of Platform 9. Near station exit. </p>"},"taxi":{"available":true,"notes":"There is taxi provision","taxiRanks":[{"covered":"The taxi rank is partially covered","deployableRamp":"Ramps can only be deployed from the side of a vehicle at this Taxi Rank","helpPoint":"There is no help point close to this taxi rank","localityOnSameSide":"The Taxi rank is on the same side as station","name":"Taxi Rank, Pancras Road","taxiConfiguration":"Taxis only pick up at this taxi rank"}]},"underground":{"available":true,"notes":"<p>N/A</p>"}}}
$kgxacc$::jsonb, NOW()),
  ('EUS', 'London Euston', 51.5282, -0.1337, 'AW', $eusacc${"carParks":{"accessibleParkingSpacesAvailable":true,"carParks":[],"numberOfAccessibleSpaces":0,"numberOfSpaces":null,"parkingSpacesAvailable":null},"cycling":{"cctv":true,"cycleHireNotes":null,"cycleStorageAvailable":true,"location":"<p>At two locations - at the west exit, near platform 16 and at the east piazza opposite M&amp;S Food (to the rear of Nandos, and at the Euston Gardens taxi rank).</p>","sheltered":true,"spaces":{"notes":null,"numberOfSpaces":415},"typesOfStorage":["Stands"]},"dropOffPickUp":{"available":true,"location":"<p>.</p>","notes":"$35","points":[{"carAccommodationCount":"There is space for up to 3 vehicles to drop-off and/or pick-up","easyToDeployRamp":null,"helpPointClose":"There is no Help Point close to the drop-off and/or pick-up point","name":"Drop off and Pick Up Point, Eversholt Street","sameSideAsStation":null,"typeOfFacility":null,"waitingAreaForDisabledPickup":"This is a combined drop-off and pick-up point"},{"carAccommodationCount":"There is space for up to 3 vehicles to drop-off and/or pick-up","easyToDeployRamp":null,"helpPointClose":"There is a Help Point close to the drop-off and/or pick-up point","name":"Drop off and Pick Up Point, Euston Square Gardens","sameSideAsStation":"The drop-off and/or pick-up point is on the same side of the road as the station","typeOfFacility":"There is seating/waiting under a canopy","waitingAreaForDisabledPickup":"This is a combined drop-off and pick-up point"}]},"helpAndSupport":{"accessibilityInformation":"There is information about accessibility","announcements":"Announcements are made both visually and audibly","customerInformationScreens":["DepartureScreens","ArrivalScreens","Announcements"],"helpPoints":{"available":true,"inductionLoop":"Induction loops are at designated listening areas","location":"Help Points are only available in limited locations","notes":"<p>A dedicated Mobility Assistance Reception is located on the main concourse adjacent to the ticket office.</p><p>Monday-Saturday 07:00-23:00Sunday 08:00-23:00</p>"},"inductionLoops":"Induction loops are at designated listening areas","informationPoints":"There are information points","printedLocalInformation":"There is printed information available","staffHelp":{"available":true,"location":"<p>.</p>","notes":"$36","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday"],"openPeriod":[{"endTime":"01:30:00.000","startTime":"04:30:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Friday","Saturday"],"openPeriod":[{"endTime":"02:00:00.000","startTime":"04:30:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"01:30:00.000","startTime":"05:15:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"Avanti West Coast Assisted Travel Contact Details","note":null,"operatorName":null,"primaryTelephoneNumber":"0345 528 0253"}},"staffInformation":["Yes - from help point","Yes - from information point","Yes - from ticket office"]},"lifts":{"available":true,"liftsInfo":[{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift has a 1.5 metre turning circle inside","name":"Lift, (West), To Concourse","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"},{"liftControls":"Lift controls should be accessible to most people","liftManoeuvrability":"The lift has a 1.5 metre turning circle inside","name":"Lift, Concourse to Mezzanine level and  First Class Lounge","openingWidth":"Lift has clear door widths at least 0.9m wide","operationalAnnouncements":"There are audio announcements provided outside and inside this lift"}],"statement":"There are lifts"},"loungesAndWaiting":{"faithRoom":"There is no multi-faith room at this station","firstClass":{"available":true,"location":"<p>.</p>","notes":"$37","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"21:00:00.000","startTime":"06:45:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Saturday"],"openPeriod":[{"endTime":"20:00:00.000","startTime":"06:45:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"20:00:00.000","startTime":"08:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":null},"firstClassLounges":[{"capableOfBeingHeated":"The First class lounge is capable of being heated and cooled","helpPointClose":null,"inSightOfCis":null,"minimumSeatingProvision":"There is seating with backrests and armrests on both sides","name":"Caledonian Sleeper First Class Lounge, Platform 1"},{"capableOfBeingHeated":"The First class lounge is capable of being heated and cooled","helpPointClose":"There is no Help Point close to the First Class lounge","inSightOfCis":"There is seating with a view of customer information screens but not trains in the First Class lounge","minimumSeatingProvision":"There is seating with backrests and armrests on both sides","name":"Avanti West Coast First Class Lounge, Mezzanine"}],"quietRoom":"There is no quiet room at this station","seatingArea":{"available":true,"location":"<p>Under the balcony on the concourse, between platforms 11 and 12, opposite platform 4/opposite Camden Food, as well as additional seating on the upstairs balcony, accessible by a lift and escalators</p>","notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"shelteredWaitingAvailable":true,"waitingFacility":{"available":true,"location":"<p>Located by the entrance to platforms 16-18</p>","notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"waitingRooms":[{"automatedDoors":"The doors to the waiting room remain open when it is open","heatingOrCoolingAvailable":"The waiting area is capable of being heated and cooled","helpPointClose":"There is no Help Point close to the waiting room","inSightOfCis":"There is seating within view of the Customer Information Screen but not of trains in the waiting room","minimumSeatingStandard":"There is seating with backrests and armrests on both sides","name":"Assisted Travel Lounge & Passenger Assist Check In, adjacent to the ticket office"},{"automatedDoors":"The doors to the waiting room are automated","heatingOrCoolingAvailable":"The waiting area is capable of being heated and cooled","helpPointClose":"There is no Help Point close to the waiting room","inSightOfCis":"There is seating within view of the Customer Information Screen but not of trains in the waiting room","minimumSeatingStandard":"There is seating with backrests and armrests on both sides","name":"Waiting room, opposite Platform 16"}]},"platformFacilities":{"entranceLevels":"The platforms are below the level of the main entrance of the station","numberOfPlatforms":16,"platforms":[{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 1","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 7","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 6","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 5","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 4","seatingAtIntervals":"Seating is limited on this platform","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 3","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 2","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 13","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 12","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 11","seatingAtIntervals":"Seating is limited on this platform","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 10","seatingAtIntervals":"There is no seating on this platform","waitingType":null},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 9","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 8","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is a Help Point close to this platform","name":"Platform 16","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 15","seatingAtIntervals":"There is seating at regular intervals of 50 metres apart or less","waitingType":"There is seating/waiting under a roof or canopy on this platform"},{"helpPointClose":"There is no Help Point close to this platform","name":"Platform 14","seatingAtIntervals":"There is seating provided at more than 50 metres apart","waitingType":"There is seating/waiting under a roof or canopy on this platform"}],"tactileWarnings":"There are tactile warnings on all platforms in use"},"staffAssistance":{"announcements":"Announcements are made both visually and audibly","customerInformation":["DepartureScreens","ArrivalScreens","Announcements"],"customerServiceNotes":"<p>Monday-Sunday 07:00-23:00&#160;  </p><p>Customers can seek information and assistance&#160;from the ticket office or the Mobility Assistance Reception.</p>","helpPoints":{"available":true,"inductionLoop":"All Help Points have induction loops","location":"Help Points are only available in limited locations","notes":"<p>A dedicated Mobility Assistance Reception is located on the main concourse adjacent to the ticket office.</p><p>Monday-Saturday 07:00-23:00Sunday 08:00-23:00</p>"},"helpline":{"available":true,"location":null,"notes":"<p>We want everyone to travel with confidence. That is why, if you are planning on travelling on national rail services, you can request an assistance booking in advance - now up to 2 hours before your journey is due to start, any time of the day. For more information about Passenger Assist and how to request an assistance booking via Passenger Assist, please <a href=\"https://www.nationalrail.co.uk/stations_destinations/passenger-assist.aspx\" title=\"\">click here</a>.</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":{"emailAddress":null,"name":"London Euston Help Line Contact Details","note":"<p>If you wish to book assistance but are not sure which train operator you are travelling with, you can call 0800 022 3720. On calling, you will be referred to the appropriate train operator.</p>","operatorName":null,"primaryTelephoneNumber":null}},"informationAvailableFromStaff":["Yes - from help point","Yes - from information point","Yes - from ticket office"],"staffHelp":{"available":true,"location":"<p>.</p>","notes":"$38","openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday"],"openPeriod":[{"endTime":"01:30:00.000","startTime":"04:30:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Friday","Saturday"],"openPeriod":[{"endTime":"02:00:00.000","startTime":"04:30:00.000"}],"openingStatus":"Specific Hours"},{"daysOfTheWeek":["Sunday"],"openPeriod":[{"endTime":"01:30:00.000","startTime":"05:15:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"Avanti West Coast Assisted Travel Contact Details","note":null,"operatorName":null,"primaryTelephoneNumber":"0345 528 0253"}}},"stationAccessibility":{"escalatorInformation":null,"inductionLoop":{"provision":"Induction loops are at designated listening areas","ticketCounters":null},"nearestAccessibleStations":null,"passengerAssistance":[{"available":true,"location":"<p>Located next to the ticket office on the main concourse. </p>","name":"London Euston Passenger Assistance Meeting Point 1","notes":null},{"available":true,"location":"<p>Help Point (Taxi Rank)</p>","name":"London Euston Passenger Assistance Meeting Point 2","notes":"<p>Located at the accessible taxi rank and Euston Station taxi rank, both equipped with Help Points to contact staff who will be able to help</p>"}],"stepFreeCategory":{"category":"A, Compliant step-free access to all platform(s)","levelAccess":null,"notes":"<p>Suitable for disabled passengers. Level access to all platforms via ramps. Lifts provided between concourse, taxi and London Underground ticket hall.</p><p>Lift access is available to the Underground&#160;ticket hall, however the Underground station itself has only escalators &amp; stairs.&#160;Nearby Euston Square station has step-free access Westbound&#160;only to the Circle, Hammersmith &amp; City and Metropolitan Lines. </p><p>For more information on the accessibility on London Transport <a href=\"https://tfl.gov.uk/transport-accessibility/\" title=\"\">click here</a>.</p>"},"tactilePaving":"There are tactile warnings on all platforms in use","ticketBarriers":{"available":true,"location":null,"names":["Ticket barrier, Platform 3","Ticket barrier, Platform 2","Ticket barrier, Platform 1","Ticket barrier, Platform 8","Ticket barrier, Platform 9","Ticket barrier, Platform 10","Ticket barrier, Platform 11"],"notes":"<p>There are ticket gates at the entrances to platforms 1 - 3 &amp; 8 - 11 and can be accessed by wide gates.</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"trainRamp":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null,"storage":"Ramps are kept at this station to provide assistance"},"wheelchairsAvailable":true},"stationFacilities":{"atm":{"available":true,"location":"<p>Located outside the station Located on concourse Located outside main entrance</p>","notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"cctvAvailable":true,"currencyExchange":null,"defibrillator":null,"limitedService":null,"lostProperty":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"17:30:00.000","startTime":"09:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"London Euston Lost Property Contact Details","note":null,"operatorName":"Excess Baggage Company","primaryTelephoneNumber":"0330 024 0215"}},"luggageStorage":{"available":true,"location":"<p> </p>","notes":null,"openingHoursNotes":null,"openingTimes":[{"daysOfTheWeek":["Monday","Tuesday","Wednesday","Thursday","Friday"],"openPeriod":[{"endTime":"23:00:00.000","startTime":"07:00:00.000"}],"openingStatus":"Specific Hours"}],"operatorContactDetails":{"emailAddress":null,"name":"London Euston Luggage Storage Contact Details","note":null,"operatorName":"Excess Baggage Company","primaryTelephoneNumber":"020 3468 4670"}},"payPhones":false,"postBox":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"refreshments":{"available":true,"location":null,"notes":"<p>Coffee shop Caf&#233; Coffee kiosk Food vending machine Public House/Bar Food outlet (Seating available) Food outlet (Seating unavailable)</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"requestStop":null,"shops":{"available":true,"location":null,"notes":"<p>Wide range of shops - see the <a href=\"https://www.nationalrail.co.uk/stations-and-destinations/stations-made-easy/london-euston-station-plan\" title=\"\">Station Plan map</a>.</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"trolleys":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"wifi":{"available":true,"location":null,"notes":"<p>Find WiFi Hotspots around London Euston station</p>","openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null}},"toiletsAndChanging":{"showers":{"available":true,"location":null,"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null},"toilets":{"accessibleToiletsAvailable":true,"available":true,"babyChangingAvailable":true,"changingPlacesToiletsAvailable":true,"location":"<p>These are located&#160;by the entrance to platforms 1-3</p><p>There are also&#160;accessible showers located in the Caledonian Sleeper lounge on platform 1 .</p>","locations":[{"accessibleHandles":null,"name":"Female Toilet, First Class lounge","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Accessible toilet, across and further from comfort lounge","radarKeyAvailable":null},{"accessibleHandles":"The toilet entry handles may be operated by most people","name":"Accessible Toilet, First Class lounge","radarKeyAvailable":"This toilet is accessed with a RADAR key"},{"accessibleHandles":null,"name":"Female Toilets, near top of ramp to Platforms 1-3","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Male Toilets, near top of ramp to Platforms 1-3","radarKeyAvailable":null},{"accessibleHandles":null,"name":"Male Toilet, First Class lounge","radarKeyAvailable":null},{"accessibleHandles":"The toilet entry handles may be operated by most people","name":"Changing Places Toilet, opposite Platform 1","radarKeyAvailable":"This toilet is accessed with a RADAR key"}],"notes":null,"openingHoursNotes":null,"openingTimes":null,"operatorContactDetails":null}},"transportLinks":{"airport":{"available":false,"notes":null},"bus":{"available":true,"notes":"<p>Buses are available from Euston bus station next to the railway station. All Transport for London buses are accessible by step free access.</p><p>More information about Transport for London buses can be found <a href=\"http://www.tfl.gov.uk/modes/buses/?cid=fs013\" title=\"\">here</a></p>"},"carHire":{"available":false,"notes":null},"port":{"available":false,"notes":null},"replacementBus":{"available":true,"maps":[],"notes":"<p>Go to Platform 2, there is a gate adjacent to the station from the main road.</p>"},"taxi":{"available":true,"notes":"There is taxi provision","taxiRanks":[{"covered":"The taxi rank is partially covered","deployableRamp":"Ramps can only be deployed from the side of a vehicle at this Taxi Rank","helpPoint":"There is no help point close to this taxi rank","localityOnSameSide":"The Taxi rank is on the same side as station","name":"Taxi rank(1), Euston Square Gardens","taxiConfiguration":"Taxis only drop off at this taxi rank"},{"covered":"The taxi rank is partially covered","deployableRamp":"Ramps can only be deployed from the side of a vehicle at this Taxi Rank","helpPoint":"There is no help point close to this taxi rank","localityOnSameSide":"The Taxi rank is via controlled roadway crossing","name":"Taxi rank(2), Euston Square Gardens","taxiConfiguration":"Taxis only pick up at this taxi rank"}]},"underground":{"available":true,"notes":"<p>Euston Underground station has access to the Northern Line and the Victoria Line.&#160;Lift access is available to the Underground&#160;ticket hall, however the Underground station itself has only escalators &amp; stairs.&#160;Nearby Euston Square station has access to the Circle, Hammersmith &amp; City and Metropolitan Lines. For more information on the accessibility on London Transport <a href=\"https://tfl.gov.uk/transport-accessibility/\" title=\"\">click here</a>.</p><p>The Underground station is very busy, especially during the morning and evening peaks. It will likely be quicker for customers to walk or catch a bus to nearby Warren Street or King&#39;s Cross St. Pancras instead.</p>"}}}
$eusacc$::jsonb, NOW())
ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility, fetched_at = EXCLUDED.fetched_at;

-- ---------------------------------------------------------------------
-- Incident
-- ---------------------------------------------------------------------
INSERT INTO incidents (
    incident_id, summary, description, operators, affected_stations,
    priority, validity_periods, is_planned, is_cleared, fetched_at,
    first_seen_at, affected_lines
)
VALUES (
    'FIXTURE-INCIDENT-1',
    'Signalling problems between London Kings Cross and Peterborough',
    'Fixture incident: due to a signalling failure, some LNER services between London Kings Cross and Peterborough are delayed by up to 20 minutes or altered. Disruption is expected to continue until end of service.',
    '{GR}', '{KGX,EUS}',
    2,
    '[{"from_date":"2026-09-17T06:00:00Z","to_date":null,"is_now":true}]'::jsonb,
    FALSE, FALSE, NOW(), NOW(), '{lner-ecml}'
)
ON CONFLICT (incident_id) DO UPDATE SET
  summary = EXCLUDED.summary, description = EXCLUDED.description,
  validity_periods = EXCLUDED.validity_periods, affected_lines = EXCLUDED.affected_lines;

-- An "Ended" incident (left the feed without RDM clearing it), so the
-- /incidents accessibility scan covers the third state badge too. See
-- docs/superpowers/specs/2026-10-06-incident-source-removal-design.md.
INSERT INTO incidents (
    incident_id, summary, description, operators, affected_stations,
    priority, validity_periods, is_planned, is_cleared, fetched_at,
    first_seen_at, affected_lines, source_missing_polls, source_removed_at
)
VALUES (
    'FIXTURE-INCIDENT-ENDED',
    'Planned engineering work between Peterborough and Grantham',
    'Fixture incident: planned engineering work the source stopped listing without clearing it.',
    '{GR}', '{}',
    4,
    '[{"from_date":"2026-09-17T22:00:00Z","to_date":null,"is_now":true}]'::jsonb,
    TRUE, FALSE, NOW() - interval '2 days', NOW() - interval '3 days', '{lner-ecml}',
    2, NOW() - interval '2 days'
)
ON CONFLICT (incident_id) DO UPDATE SET
  summary = EXCLUDED.summary, description = EXCLUDED.description,
  validity_periods = EXCLUDED.validity_periods, affected_lines = EXCLUDED.affected_lines,
  fetched_at = EXCLUDED.fetched_at, source_missing_polls = EXCLUDED.source_missing_polls,
  source_removed_at = EXCLUDED.source_removed_at;

-- An "Upcoming" incident: listed and uncleared, but its only validity
-- window is next week, so it is not in effect yet (2026-10-20,
-- docs/superpowers/specs/2026-10-20-incident-validity-state-design.md).
-- The window is relative to NOW() so it stays upcoming. First seen on a
-- fixed date more than 30 days before both today and visual.spec.ts's
-- FIXED_NOW (2026-09-17), so /incidents' default 30 days (and its
-- screenshot) leaves it out; `period=all` shows it. No "signal" or
-- "engineering" in its text: list-filters.spec.ts searches for both.
INSERT INTO incidents (
    incident_id, summary, description, operators, affected_stations,
    priority, validity_periods, is_planned, is_cleared, fetched_at,
    first_seen_at, affected_lines
)
VALUES (
    'FIXTURE-INCIDENT-UPCOMING',
    'Buses replace trains between Middlesbrough and Saltburn next week',
    'Fixture incident: buses replace trains on one day next week.',
    '{NT}', '{}',
    2,
    jsonb_build_array(jsonb_build_object(
        'from_date', to_char((NOW() + interval '5 days') AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
        'to_date', to_char((NOW() + interval '6 days') AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
        'is_now', false)),
    TRUE, FALSE, NOW(), '2026-07-01T09:00:00Z', '{}'
)
ON CONFLICT (incident_id) DO UPDATE SET
  summary = EXCLUDED.summary, description = EXCLUDED.description,
  validity_periods = EXCLUDED.validity_periods, affected_lines = EXCLUDED.affected_lines,
  fetched_at = EXCLUDED.fetched_at, first_seen_at = EXCLUDED.first_seen_at;

-- ---------------------------------------------------------------------
-- Shared train identity + a pinned RESOLVED subscription (the second
-- train_subscriptions row -- see the unresolved one's comment above for why
-- order matters) + live state + one event
-- ---------------------------------------------------------------------
-- The origin platform (`trains.platform`/`planned_platform`) has changed,
-- 6 to 9, so the train page shows a changed platform badge (6 struck
-- through, then 9; `PlatformBadge`) in both the progress line and the
-- calling-point table: e2e/journey-timeline.spec.ts checks no badge is
-- cut short at phone widths.
INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure, destination_crs, calling_points, matched_line_id, schedule_matched_at, train_id, resolved_at, created_at, platform, planned_platform)
VALUES (
    'W12345', '2026-09-17', 'KGX', '2026-09-17T09:00:00Z', 'EDB',
    '[{"tiploc":"KNGX","kind":"Origin","bookedArrival":null,"bookedDeparture":"09:00:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"YORK","kind":"Intermediate","bookedArrival":"10:55:00","bookedDeparture":"10:57:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"EDINBUR","kind":"Terminate","bookedArrival":"13:30:00","bookedDeparture":null,"isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0}]'::jsonb,
    'lner-ecml', NOW(), 'TRUST-FIXTURE-1', NOW(), NOW(), '9', '6'
)
ON CONFLICT (train_uid, service_date) DO UPDATE SET
  train_id = EXCLUDED.train_id, calling_points = EXCLUDED.calling_points,
  matched_line_id = EXCLUDED.matched_line_id, schedule_matched_at = EXCLUDED.schedule_matched_at,
  resolved_at = EXCLUDED.resolved_at,
  platform = EXCLUDED.platform, planned_platform = EXCLUDED.planned_platform
RETURNING id;

-- The TIPLOC to CRS rows for the train's three calling points, without
-- which the page's calling-point table has a header and no rows. Fixture
-- STANOX codes (99xxx, no real location has one).
INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('99001', 'KGX', 'KNGX', 'London Kings Cross', 1),
  ('99002', 'YRK', 'YORK', 'York', 1),
  ('99003', 'EDB', 'EDINBUR', 'Edinburgh', 1)
ON CONFLICT DO NOTHING;

-- Capture the trains.id we just created/updated for reuse below via a CTE-based insert.
WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W12345' AND service_date = '2026-09-17'
)
INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, pin_operator, resolution_status, tracked_at, trains_id, notifications_enabled)
SELECT 'fixture-owner', '2026-09-17', 'KGX', '2026-09-17T09:00:00Z', 'EDB', 'GR', 'resolved', NOW(), t.id, TRUE
FROM t
ON CONFLICT DO NOTHING;

WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W12345' AND service_date = '2026-09-17'
)
INSERT INTO train_current_state (status, last_reported_location, last_event_type, delay_minutes, next_calling_point, eta_next, eta_source, updated_at, trains_id, event_time)
SELECT 'en_route', 'York', 'DEPARTURE', 4, 'Edinburgh Waverley', '2026-09-17T13:34:00Z', 'trust-propagated', NOW(), t.id, NOW()
FROM t
ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET status = EXCLUDED.status, updated_at = EXCLUDED.updated_at;

WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W12345' AND service_date = '2026-09-17'
)
INSERT INTO train_movement_events (dedup_key, msg_type, event_type, loc_stanox, loc_crs, planned_timestamp, actual_timestamp, variation_status, raw_body, received_at, trains_id)
SELECT 'FIXTURE-DEDUP-1', '0002', 'DEPARTURE', '52let more', 'YRK', '2026-09-17T10:57:00Z', '2026-09-17T11:01:00Z', 'LATE', '{}'::jsonb, NOW(), t.id
FROM t
ON CONFLICT DO NOTHING;

-- A standalone journey ticket attached to this train subscription.
WITH sub AS (
  SELECT ts.id FROM train_subscriptions ts
  JOIN trains t ON t.id = ts.trains_id
  WHERE t.train_uid = 'W12345' AND t.service_date = '2026-09-17' AND ts.user_id = 'fixture-owner'
)
INSERT INTO tracked_train_tickets (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs, source, created_at)
SELECT sub.id, 'fixture-owner', 'LNER', 'single', 'KGX', 'EDB', 'manual', NOW()
FROM sub
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- A train tracked by UID with a timetable and no Network Rail report
-- yet, on a far-future date: the api shows it as planned (trackingPhase,
-- docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
-- §4) until 45 minutes before its 07:10 departure from King's Cross, so
-- always. tracked-train-states.spec.ts finds it by its UID. The
-- unresolved pin above (no timetable match, no report) is the spec's
-- "not in the timetable" train.
-- ---------------------------------------------------------------------
INSERT INTO trains (train_uid, service_date, created_at)
VALUES ('W77777', '2099-01-05', NOW())
ON CONFLICT (train_uid, service_date) DO NOTHING;

INSERT INTO schedule_destination_departures (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
VALUES ('2099-01-05', 'EDB', '07:10', 'W77777', 'KGX', 'KGX')
ON CONFLICT DO NOTHING;

WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W77777' AND service_date = '2099-01-05'
)
INSERT INTO train_subscriptions (user_id, service_date, resolution_status, tracked_at, trains_id, notifications_enabled)
SELECT 'fixture-owner', '2099-01-05', 'pending', NOW(), t.id, TRUE
FROM t
WHERE NOT EXISTS (SELECT 1 FROM train_subscriptions ts WHERE ts.trains_id = t.id);

-- A journey for that train with an untracked walk to the station before
-- it (the same spec, §2): the first `journeys` row, so `id = 1` on a fresh
-- database, like E2E_TRACKING_ID above (visual.spec.ts's `journey` shot).
-- In 2099, so the walk is always "Planned". Named, so its title doesn't
-- depend on how journeys are named by default; its preview line starts
-- from the walk's place ("Islington"), which has no station code.
INSERT INTO journeys (user_id, custom_name, created_at, updated_at)
SELECT 'fixture-owner', 'Fixture door to door', NOW(), NOW()
WHERE NOT EXISTS (SELECT 1 FROM journeys WHERE custom_name = 'Fixture door to door');

WITH j AS (
  SELECT id FROM journeys WHERE custom_name = 'Fixture door to door'
)
INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, transit_mode, origin_place, destination_place, planned_departure, planned_arrival)
SELECT j.id, 1, '2099-01-05', 'manual', 'untracked', 'walk', 'Islington', 'London Kings Cross', '2099-01-05T06:30:00Z', '2099-01-05T06:50:00Z'
FROM j
ON CONFLICT (journey_id, leg_order) DO NOTHING;

WITH j AS (
  SELECT id FROM journeys WHERE custom_name = 'Fixture door to door'
), sub AS (
  SELECT ts.id FROM train_subscriptions ts
  JOIN trains t ON t.id = ts.trains_id
  WHERE t.train_uid = 'W77777' AND t.service_date = '2099-01-05' AND ts.user_id = 'fixture-owner'
)
INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, train_subscription_id, match_mode)
SELECT j.id, 2, 'KGX', 'EDB', '2099-01-05', sub.id, 'manual'
FROM j, sub
ON CONFLICT (journey_id, leg_order) DO NOTHING;

-- ---------------------------------------------------------------------
-- Group with fixture-owner as owner and fixture-member as member, plus
-- an active invite link and the tracked train shared into it.
-- ---------------------------------------------------------------------
INSERT INTO groups (id, name, created_by, created_at)
VALUES ('fixture-group-1', 'Fixture Family', 'fixture-owner', NOW())
ON CONFLICT (id) DO NOTHING;

INSERT INTO group_members (group_id, user_id, role, joined_at)
VALUES
  ('fixture-group-1', 'fixture-owner', 'owner', NOW()),
  ('fixture-group-1', 'fixture-member', 'member', NOW())
ON CONFLICT (group_id, user_id) DO NOTHING;

-- The invite token is stored hashed (`token_hash`, migration
-- 20260926150000_hash_share_and_invite_tokens.sql): the RAW token is
-- E2E_GROUP_INVITE_TOKEN in ci.yml, and this row stores its SHA-256 lowercase
-- hex digest -- the same value auth::hash_session_token (and so
-- unlisted_links::hash_link_token) produces when the api looks it up.
INSERT INTO group_invite_links (token_hash, group_id, created_by, created_at, expires_at)
VALUES (encode(sha256(convert_to('fixture-invite-token-123456789', 'UTF8')), 'hex'), 'fixture-group-1', 'fixture-owner', NOW(), NOW() + INTERVAL '7 days')
ON CONFLICT (token_hash) DO NOTHING;

WITH sub AS (
  SELECT ts.id FROM train_subscriptions ts
  JOIN trains t ON t.id = ts.trains_id
  WHERE t.train_uid = 'W12345' AND t.service_date = '2026-09-17' AND ts.user_id = 'fixture-owner'
)
INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at)
SELECT 'fixture-group-1', sub.id, 'fixture-owner', NOW()
FROM sub
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- What fixture-member shares into the group (trackingPhase on group and
-- shared rows, 2026-10-20): their own tracking of the planned W77777, a
-- timetabled bus (W88888) and a walk-and-Tube journey with no train, all
-- in 2099. The owner sees the two trains as shared rows on /journeys
-- ("Planned", "Bus service" + "Timetabled only", as on their own rows)
-- and everything on /groups/fixture-group-1, with the owner's
-- door-to-door journey above shared in too. Inserted after every row
-- whose id a spec reads (E2E_TRACKING_ID, E2E_PLANNED_TRACKING_ID,
-- E2E_JOURNEY_ID), so those ids don't move.
-- ---------------------------------------------------------------------
INSERT INTO trains (train_uid, service_date, created_at)
VALUES ('W88888', '2099-01-05', NOW())
ON CONFLICT (train_uid, service_date) DO NOTHING;

INSERT INTO schedule_destination_departures (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs)
VALUES ('2099-01-05', 'EUS', '09:00', 'W88888', 'KGX', 'KGX')
ON CONFLICT DO NOTHING;

INSERT INTO schedule_services (service_date, uid, mode, stp)
VALUES ('2099-01-05', 'W88888', 'bus', 'P')
ON CONFLICT DO NOTHING;

INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_destination_crs, resolution_status, tracked_at, trains_id, notifications_enabled)
SELECT 'fixture-member', '2099-01-05', 'KGX', v.destination, v.resolution, NOW(), t.id, TRUE
FROM (VALUES ('W77777', 'EDB', 'pending'), ('W88888', 'EUS', 'schedule_matched')) AS v(uid, destination, resolution)
JOIN trains t ON t.train_uid = v.uid AND t.service_date = '2099-01-05'
WHERE NOT EXISTS (
  SELECT 1 FROM train_subscriptions ts WHERE ts.trains_id = t.id AND ts.user_id = 'fixture-member'
)
ORDER BY v.uid;

INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at)
SELECT 'fixture-group-1', ts.id, 'fixture-member', NOW()
FROM train_subscriptions ts
JOIN trains t ON t.id = ts.trains_id
WHERE ts.user_id = 'fixture-member' AND t.train_uid IN ('W77777', 'W88888') AND t.service_date = '2099-01-05'
ON CONFLICT DO NOTHING;

INSERT INTO journeys (user_id, custom_name, created_at, updated_at)
SELECT 'fixture-member', 'Fixture walk and Tube', NOW(), NOW()
WHERE NOT EXISTS (SELECT 1 FROM journeys WHERE custom_name = 'Fixture walk and Tube');

WITH j AS (
  SELECT id FROM journeys WHERE custom_name = 'Fixture walk and Tube'
)
INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, transit_mode, line_name, origin_place, destination_place, planned_departure, planned_arrival)
SELECT j.id, v.leg_order, '2099-01-05', 'manual', 'untracked', v.mode, v.line, v.origin, v.destination, v.departure::timestamptz, v.arrival::timestamptz
FROM j, (VALUES
  (1, 'walk', NULL, 'Islington', 'Angel', '2099-01-05T08:00:00Z', '2099-01-05T08:10:00Z'),
  (2, 'tube', 'Northern line', 'Angel', 'Bank', '2099-01-05T08:12:00Z', '2099-01-05T08:20:00Z')
) AS v(leg_order, mode, line, origin, destination, departure, arrival)
ON CONFLICT (journey_id, leg_order) DO NOTHING;

INSERT INTO group_journeys (group_id, journey_id, added_by, added_at)
SELECT 'fixture-group-1', j.id, j.user_id, NOW()
FROM journeys j
WHERE j.custom_name IN ('Fixture door to door', 'Fixture walk and Tube')
ON CONFLICT DO NOTHING;

-- Two past journeys of fixture-member's, shared into the same group, for
-- the Journeys page's merged history (DS-178, journeys-page.spec.ts): one
-- the day after fixture-owner's planned journey 900001 (2026-09-17) and one
-- a week before, so its Past list reads shared, own, shared, newest first.
-- Fixed ids well clear of the serial ones, fixed dates long gone, nobody
-- follows them, and walks have no train for any other spec to meet.
INSERT INTO journeys (id, user_id, custom_name, created_at, updated_at)
VALUES
  (900101, 'fixture-member', 'Fixture shared later walk', NOW(), NOW()),
  (900102, 'fixture-member', 'Fixture shared earlier walk', NOW(), NOW())
ON CONFLICT (id) DO NOTHING;

INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, transit_mode, origin_place, destination_place, planned_departure, planned_arrival)
VALUES
  (900101, 1, '2026-09-18', 'manual', 'untracked', 'walk', 'Islington', 'Angel', '2026-09-18T08:00:00Z', '2026-09-18T08:20:00Z'),
  (900102, 1, '2026-09-10', 'manual', 'untracked', 'walk', 'Angel', 'Islington', '2026-09-10T17:00:00Z', '2026-09-10T17:20:00Z')
ON CONFLICT (journey_id, leg_order) DO NOTHING;

INSERT INTO group_journeys (group_id, journey_id, added_by, added_at)
VALUES
  ('fixture-group-1', 900101, 'fixture-member', NOW()),
  ('fixture-group-1', 900102, 'fixture-member', NOW())
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- Transit: a small Manchester Metrolink network (transit networks spec;
-- the pages under /transit, shown because ci.yml's frontend-e2e job sets
-- API_TRANSIT_NETWORKS=metrolink). Two lines, three stations, a few trams
-- on 2026-09-17 (the visual tests pass ?date=2026-09-17) and today (so a
-- page without a date shows a board too), and one replacement bus. The
-- horizon runs from the fixed date to 60 days after the seeding.
-- ---------------------------------------------------------------------
INSERT INTO transit_feed_versions
  (network, feed_version, state, network_row, rail_links, content_sha256, counts,
   horizon_from, horizon_to, source, staged_at, produced_at, activated_at)
VALUES ('metrolink', 'e2e-v1', 'active',
  '{"name": "Manchester Metrolink", "mode": "tram", "timezone": "Europe/London", "attribution": {"licence": "OGL-UK-3.0", "text": "Contains public sector information licensed under the Open Government Licence v3.0. Timetables and disruptions: Bus Open Data Service (Department for Transport). Stops: NaPTAN (Department for Transport)."}}',
  '[]', repeat('e', 64), '{"lines": 2, "stops": 6, "patterns": 3, "trips": 5, "service_days": 2}',
  '2026-09-01', CURRENT_DATE + 60, '{"url_kind": "e2e", "feed_version": null, "published_at": null}',
  NOW(), NOW(), NOW())
ON CONFLICT (network, feed_version) DO NOTHING;

INSERT INTO transit_networks
  (network, name, mode, timezone, attribution, active_feed_version, activated_at, updated_at)
VALUES ('metrolink', 'Manchester Metrolink', 'tram', 'Europe/London',
  '{"licence": "OGL-UK-3.0", "text": "Contains public sector information licensed under the Open Government Licence v3.0. Timetables and disruptions: Bus Open Data Service (Department for Transport). Stops: NaPTAN (Department for Transport)."}',
  'e2e-v1', NOW(), NOW())
ON CONFLICT (network) DO UPDATE SET active_feed_version = EXCLUDED.active_feed_version;

INSERT INTO transit_lines (network, feed_version, line_id, name, colour, sort_order, directions) VALUES
  ('metrolink', 'e2e-v1', 'blue', 'Blue Line', '#1f6fb5', 1,
   '[{"direction": "outbound", "headsign": "Eccles", "stations": ["9400ZZMAPIC", "9400ZZMAGMX", "9400ZZMAECC"]},
     {"direction": "inbound", "headsign": "Piccadilly", "stations": ["9400ZZMAECC", "9400ZZMAGMX", "9400ZZMAPIC"]}]'),
  ('metrolink', 'e2e-v1', 'green', 'Green Line', '#2e8b3e', 2,
   '[{"direction": "outbound", "headsign": "Deansgate-Castlefield", "stations": ["9400ZZMAPIC", "9400ZZMAGMX"]}]')
ON CONFLICT DO NOTHING;

INSERT INTO transit_stops (network, feed_version, stop_id, kind, parent_stop_id, name, indicator, lat, lon, wheelchair) VALUES
  ('metrolink', 'e2e-v1', '9400ZZMAPIC', 'station', NULL, 'Piccadilly', NULL, 53.4774, -2.2309, true),
  ('metrolink', 'e2e-v1', '9400ZZMAPIC1', 'platform', '9400ZZMAPIC', 'Piccadilly', 'To Eccles', NULL, NULL, NULL),
  ('metrolink', 'e2e-v1', '9400ZZMAGMX', 'station', NULL, 'Deansgate-Castlefield', NULL, 53.4746, -2.2507, true),
  ('metrolink', 'e2e-v1', '9400ZZMAGMX1', 'platform', '9400ZZMAGMX', 'Deansgate-Castlefield', 'To Eccles', NULL, NULL, NULL),
  ('metrolink', 'e2e-v1', '9400ZZMAECC', 'station', NULL, 'Eccles', NULL, 53.4836, -2.3343, true),
  ('metrolink', 'e2e-v1', '9400ZZMAECC1', 'platform', '9400ZZMAECC', 'Eccles', 'To City', NULL, NULL, NULL)
ON CONFLICT DO NOTHING;

INSERT INTO transit_patterns (network, feed_version, pattern_id, line_id, direction, headsign) VALUES
  ('metrolink', 'e2e-v1', 'p1', 'blue', 0, 'Eccles'),
  ('metrolink', 'e2e-v1', 'p2', 'green', 0, 'Deansgate-Castlefield'),
  ('metrolink', 'e2e-v1', 'p3', NULL, 0, 'Eccles')
ON CONFLICT DO NOTHING;

INSERT INTO transit_pattern_stops (network, feed_version, pattern_id, seq, stop_id, arr_offset_s, dep_offset_s, pickup, drop_off) VALUES
  ('metrolink', 'e2e-v1', 'p1', 0, '9400ZZMAPIC1', 0, 0, 0, 1),
  ('metrolink', 'e2e-v1', 'p1', 1, '9400ZZMAGMX1', 240, 240, 0, 0),
  ('metrolink', 'e2e-v1', 'p1', 2, '9400ZZMAECC1', 1500, 1500, 1, 0),
  ('metrolink', 'e2e-v1', 'p2', 0, '9400ZZMAPIC1', 0, 0, 0, 1),
  ('metrolink', 'e2e-v1', 'p2', 1, '9400ZZMAGMX1', 240, 240, 1, 0),
  ('metrolink', 'e2e-v1', 'p3', 0, '9400ZZMAPIC1', 0, 0, 0, 1),
  ('metrolink', 'e2e-v1', 'p3', 1, '9400ZZMAECC1', 2400, 2400, 1, 0)
ON CONFLICT DO NOTHING;

INSERT INTO transit_trips (network, feed_version, trip_id, line_id, pattern_id, service_id, start_s, headsign, replacement_bus) VALUES
  ('metrolink', 'e2e-v1', 'E2E-0702', 'blue', 'p1', 's1', 25320, 'Eccles', false),
  ('metrolink', 'e2e-v1', 'E2E-0714', 'blue', 'p1', 's1', 26040, 'Eccles', false),
  ('metrolink', 'e2e-v1', 'E2E-0706', 'green', 'p2', 's1', 25560, 'Deansgate-Castlefield', false),
  ('metrolink', 'e2e-v1', 'E2E-0730', NULL, 'p3', 's1', 27000, 'Eccles', true),
  ('metrolink', 'e2e-v1', 'E2E-0802', 'blue', 'p1', 's1', 28920, 'Eccles', false)
ON CONFLICT DO NOTHING;

INSERT INTO transit_service_days (network, feed_version, service_id, service_date) VALUES
  ('metrolink', 'e2e-v1', 's1', '2026-09-17'),
  ('metrolink', 'e2e-v1', 's1', CURRENT_DATE)
ON CONFLICT DO NOTHING;

-- Metrolink disruptions (transit plan 4.8): a delay on the Blue Line, a
-- lift out of service at Piccadilly (a stop notice), and works on the
-- Green Line three days from the seeding (upcoming), with the line
-- statuses the writer would derive from them. The situations' freshness
-- marker is stamped 12 hours ahead of the seeding, so the pages read the
-- disruptions as current however long the e2e job runs before its
-- screenshots (they turn "unavailable" 30 minutes after the marker). Only
-- this fixture does that; the writer never trusts a future marker.
INSERT INTO ingest_freshness (source, fetched_at)
VALUES ('transit-metrolink-situations', NOW() + interval '12 hours')
ON CONFLICT (source) DO UPDATE SET fetched_at = EXCLUDED.fetched_at;

INSERT INTO transit_situations
  (network, situation_id, version, progress, planned, reason, summary, description, info_url,
   validity, publication, consequences, line_ids, stop_ids, scope, observed_at, first_seen_at)
VALUES
  ('metrolink', 'E2E-S1', 1, 'open', false, 'equipment:pointsFailure',
   'Delays between Piccadilly and Eccles', 'A points fault at Cornbrook is causing delays.', NULL,
   '[{"start": "2026-09-01T05:00:00Z", "end": null}]', '[]',
   '[{"condition": "delayed", "severity": "slight", "all_lines": false, "line_ids": ["blue"], "unmapped_line_refs": [], "stop_ids": [], "advice": "Allow extra time."}]',
   '{blue}', '{}', 'line', NOW(), NOW()),
  ('metrolink', 'E2E-S2', 1, 'open', false, 'equipment:liftFailure',
   'Lift out of service at Piccadilly', 'Use the ramp from the main concourse.', NULL,
   '[{"start": "2026-09-01T05:00:00Z", "end": null}]', '[]',
   '[{"condition": "unknown", "severity": "slight", "all_lines": true, "line_ids": [], "unmapped_line_refs": [], "stop_ids": ["9400ZZMAPIC"], "advice": null}]',
   '{}', '{9400ZZMAPIC}', 'stop', NOW(), NOW()),
  ('metrolink', 'E2E-S3', 1, 'open', true, 'miscellaneous:engineeringWorks',
   'No trams to Deansgate-Castlefield', NULL, NULL,
   jsonb_build_array(jsonb_build_object(
     'start', to_char((CURRENT_DATE + 3)::timestamp + interval '5 hours', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
     'end', to_char((CURRENT_DATE + 4)::timestamp + interval '23 hours', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'))),
   '[]',
   '[{"condition": "suspended", "severity": "severe", "all_lines": false, "line_ids": ["green"], "unmapped_line_refs": [], "stop_ids": [], "advice": "Use replacement buses."}]',
   '{green}', '{}', 'line', NOW(), NOW())
ON CONFLICT DO NOTHING;

INSERT INTO line_status (line_id, name, mode_name, operators, statuses, upcoming, computed_at, source, source_updated_at)
VALUES
  ('metrolink:blue', 'Blue Line', 'metrolink', '{}',
   '[{"severity": 9, "reason": "Delays between Piccadilly and Eccles", "validity": {"from_date": "2026-09-01T05:00:00Z", "to_date": null, "is_now": true}, "disruption": {"category": "RealTime", "description": "A points fault at Cornbrook is causing delays.", "affected_stops": [], "source": "transit-situation-E2E-S1"}, "data_quality": "transit"}]',
   '[]', NOW(), 'transit', NOW()),
  ('metrolink:green', 'Green Line', 'metrolink', '{}',
   '[{"severity": 27, "reason": "No disruption has been published for this line.", "validity": {"from_date": "2026-09-01T05:00:00Z", "to_date": null, "is_now": true}, "data_quality": "transit"}]',
   jsonb_build_array(jsonb_build_object(
     'from', to_char((CURRENT_DATE + 3)::timestamp + interval '5 hours', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
     'to', to_char((CURRENT_DATE + 4)::timestamp + interval '23 hours', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
     'summary', 'No trams to Deansgate-Castlefield',
     'incident_id', 'E2E-S3')),
   NOW(), 'transit', NOW())
ON CONFLICT (line_id) DO NOTHING;

-- Piccadilly's link to National Rail (transit plan 5.1). MAN has no row in
-- `stations` here, so the stop page names it by its code.
INSERT INTO transit_rail_links (network, stop_id, crs, tiploc, kind, note, observed_at)
VALUES ('metrolink', '9400ZZMAPIC', 'MAN', 'MNCRPIC', 'interchange', NULL, NOW())
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- A tiny timetable for /plan (plan.spec.ts and the plan screenshots in
-- visual.spec.ts), on the seed's service date. Made-up station codes (ZEA,
-- ZEC) and TIPLOCs, so no other page's data changes: they are in
-- `stanox_crs` only, not `stations`.
--   E2EPL3  ZEA 07:30 -> ZEC 08:20
--   E2EPL1  ZEA 08:00 -> ZEC 08:50
--   E2EPL2  ZEA 08:30 -> ZEC 09:20
-- and one on the next service date, 2026-09-18, for the overnight search
-- (both dates' trains: `acrossMidnight`):
--   E2EPL4  ZEA 00:10 -> ZEC 00:40
-- ---------------------------------------------------------------------
INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('E2EPLAN-ZEA', 'ZEA', 'E2EPLA', 'Fixture Junction', 1),
  ('E2EPLAN-ZEC', 'ZEC', 'E2EPLC', 'Fixture Central', 1)
ON CONFLICT (stanox) DO NOTHING;

INSERT INTO schedule_calling_points_full
  (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset)
VALUES
  ('2026-09-17', 'E2EPL3', 0, 'E2EPLA', 'origin', NULL, '07:30:00', 0),
  ('2026-09-17', 'E2EPL3', 1, 'E2EPLC', 'terminate', '08:20:00', NULL, 0),
  ('2026-09-17', 'E2EPL1', 0, 'E2EPLA', 'origin', NULL, '08:00:00', 0),
  ('2026-09-17', 'E2EPL1', 1, 'E2EPLC', 'terminate', '08:50:00', NULL, 0),
  ('2026-09-17', 'E2EPL2', 0, 'E2EPLA', 'origin', NULL, '08:30:00', 0),
  ('2026-09-17', 'E2EPL2', 1, 'E2EPLC', 'terminate', '09:20:00', NULL, 0),
  ('2026-09-18', 'E2EPL4', 0, 'E2EPLA', 'origin', NULL, '00:10:00', 0),
  ('2026-09-18', 'E2EPL4', 1, 'E2EPLC', 'terminate', '00:40:00', NULL, 0)
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- Two routes for /plan's Compare options (DS-196; plan.spec.ts), on the
-- seed's service date: the fast one calls at ZOS, the other at ZOC, 20
-- minutes slower, within the alternatives' detour bound.
--   E2EOP1  ZOA 08:00 -> ZOS 08:30 -> ZOD 09:00
--   E2EOP2  ZOA 08:05 -> ZOC 08:40 -> ZOD 09:20
-- Made-up codes in `stanox_crs` only, as above.
-- ---------------------------------------------------------------------
INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('E2EOPT-ZOA', 'ZOA', 'E2EOPA', 'Fixture Options', 1),
  ('E2EOPT-ZOS', 'ZOS', 'E2EOPS', 'Fixture Sidings', 1),
  ('E2EOPT-ZOC', 'ZOC', 'E2EOPC', 'Fixture Crossing', 1),
  ('E2EOPT-ZOD', 'ZOD', 'E2EOPD', 'Fixture Docks', 1)
ON CONFLICT (stanox) DO NOTHING;

INSERT INTO schedule_calling_points_full
  (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset)
VALUES
  ('2026-09-17', 'E2EOP1', 0, 'E2EOPA', 'origin', NULL, '08:00:00', 0),
  ('2026-09-17', 'E2EOP1', 1, 'E2EOPS', 'intermediate', '08:30:00', '08:31:00', 0),
  ('2026-09-17', 'E2EOP1', 2, 'E2EOPD', 'terminate', '09:00:00', NULL, 0),
  ('2026-09-17', 'E2EOP2', 0, 'E2EOPA', 'origin', NULL, '08:05:00', 0),
  ('2026-09-17', 'E2EOP2', 1, 'E2EOPC', 'intermediate', '08:40:00', '08:41:00', 0),
  ('2026-09-17', 'E2EOP2', 2, 'E2EOPD', 'terminate', '09:20:00', NULL, 0)
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- A rail, tram, rail route for /plan (transit trip planning, DS-164;
-- plan.spec.ts, with ci.yml's API_TRIP_PLAN_TRANSIT_NETWORKS=metrolink),
-- on the seed's service date, riding the Blue Line tram already seeded
-- above (E2E-0702: Piccadilly 07:02 to Eccles 07:27):
--   E2ETR1  ZTA 06:20 -> MAN 06:50, then a walk to Piccadilly's tram stop
--   E2ETR2  ZTE 07:45 -> ZTB 08:15, after a walk from Eccles's tram stop
-- Made-up codes (ZTA, ZTB, ZTE) in `stanox_crs` only, and MAN's TIPLOC
-- (MAN has no `stations` row here, as above), so no page shows them. The
-- Eccles tram stop's link to ZTE is not on any screenshot.
-- ---------------------------------------------------------------------
INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('E2ETRAM-ZTA', 'ZTA', 'E2ETRA', 'Fixture Tramway Junction', 1),
  ('E2ETRAM-ZTB', 'ZTB', 'E2ETRB', 'Fixture Tramway Central', 1),
  ('E2ETRAM-ZTE', 'ZTE', 'E2ETRE', 'Fixture Eccles Road', 1),
  ('E2ETRAM-MAN', 'MAN', 'MNCRPIC', 'MANCHESTER PICCADILLY', 1)
ON CONFLICT (stanox) DO NOTHING;

INSERT INTO schedule_calling_points_full
  (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset)
VALUES
  ('2026-09-17', 'E2ETR1', 0, 'E2ETRA', 'origin', NULL, '06:20:00', 0),
  ('2026-09-17', 'E2ETR1', 1, 'MNCRPIC', 'terminate', '06:50:00', NULL, 0),
  ('2026-09-17', 'E2ETR2', 0, 'E2ETRE', 'origin', NULL, '07:45:00', 0),
  ('2026-09-17', 'E2ETR2', 1, 'E2ETRB', 'terminate', '08:15:00', NULL, 0)
ON CONFLICT DO NOTHING;

INSERT INTO transit_rail_links (network, stop_id, crs, tiploc, kind, note, observed_at)
VALUES ('metrolink', '9400ZZMAECC', 'ZTE', 'E2ETRE', 'interchange', NULL, NOW())
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- A PLANNED three-leg journey for fixture-owner (journey-names.spec.ts and
-- the logged-in /journeys screenshot in visual.spec.ts): no leg has a
-- train yet, so there is no tracking data at all. Its row on /journeys
-- must name it from the first leg's origin (EUS) to the LAST leg's
-- destination (KGX), not the first leg's (MKC). EUS and KGX are the two
-- seeded `stations` rows, so both ends have names; MKC and PBO are not.
-- A fixed id far above the sequence so a spec's own POST /Journeys never
-- meets it (E2E_PLANNED_JOURNEY_ID, default 900001).
-- ---------------------------------------------------------------------
INSERT INTO journeys (id, user_id, custom_name, created_at, updated_at)
VALUES (900001, 'fixture-owner', NULL, NOW(), NOW())
ON CONFLICT (id) DO NOTHING;

INSERT INTO journey_legs
  (journey_id, leg_order, origin_crs, destination_crs, service_date, depart_after, match_mode)
VALUES
  (900001, 1, 'EUS', 'MKC', '2026-09-17', '08:00', 'unmatched'),
  (900001, 2, 'MKC', 'PBO', '2026-09-17', '10:00', 'unmatched'),
  (900001, 3, 'PBO', 'KGX', '2026-09-17', '12:00', 'unmatched')
ON CONFLICT (journey_id, leg_order) DO NOTHING;

-- ---------------------------------------------------------------------
-- Three trains TOMORROW for the mid-journey changes flow
-- (journey-mid-change.spec.ts: "I missed this train", take another, Undo),
-- which needs trains still to run. "Tomorrow" is London's, as the spec
-- computes it: the database's CURRENT_DATE is UTC, a day behind London
-- from 23:00 to midnight UTC while BST is in force. The day after is
-- seeded too, so a run that crosses London midnight after seeding still
-- finds its trains. Made-up codes (ZMA, ZMB) and TIPLOCs in
-- `stanox_crs` only, as the /plan fixture above, so no other page changes.
--   E2EMJ1  ZMA 09:00 -> ZMB 09:40
--   E2EMJ2  ZMA 09:30 -> ZMB 10:10
--   E2EMJ3  ZMA 10:00 -> ZMB 10:40
-- ---------------------------------------------------------------------
INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('E2EMJ-ZMA', 'ZMA', 'E2EMJA', 'Fixture Halt', 1),
  ('E2EMJ-ZMB', 'ZMB', 'E2EMJB', 'Fixture Parkway', 1)
ON CONFLICT (stanox) DO NOTHING;

INSERT INTO schedule_calling_points_full
  (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure,
   public_arrival, public_departure, day_offset, can_board, can_alight)
SELECT d.service_date, uid, seq, tiploc, kind, arr::time, dep::time, arr::time, dep::time, 0,
       TRUE, TRUE
FROM (SELECT (NOW() AT TIME ZONE 'Europe/London')::date + n AS service_date
      FROM (VALUES (1), (2)) AS o (n)) AS d
CROSS JOIN (VALUES
  ('E2EMJ1', 0, 'E2EMJA', 'origin', NULL, '09:00'),
  ('E2EMJ1', 1, 'E2EMJB', 'terminate', '09:40', NULL),
  ('E2EMJ2', 0, 'E2EMJA', 'origin', NULL, '09:30'),
  ('E2EMJ2', 1, 'E2EMJB', 'terminate', '10:10', NULL),
  ('E2EMJ3', 0, 'E2EMJA', 'origin', NULL, '10:00'),
  ('E2EMJ3', 1, 'E2EMJB', 'terminate', '10:40', NULL)
) AS t (uid, seq, tiploc, kind, arr, dep)
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- The train finder's operator filter and train day (list-filters.spec.ts):
-- at a station nothing else uses (ZFX), on the seed's 2099-01-05, two c2c
-- departures either side of midnight -- the 00:30 is still 2099-01-05's
-- train (day_offset 1) -- and one with no operator.
-- ---------------------------------------------------------------------
INSERT INTO schedule_destination_departures
  (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs,
   day_offset, public_departure, operator_atoc)
VALUES
  ('2099-01-05', 'KGX', '22:30', 'W61001', 'ZFX', 'ZFX', 0, '22:30', 'CC'),
  ('2099-01-05', 'KGX', '23:00', 'W61002', 'ZFX', 'ZFX', 0, '23:00', NULL),
  ('2099-01-05', 'KGX', '00:30', 'W61003', 'ZFX', 'ZFX', 1, '00:30', 'CC')
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- Two TfL stops for the /tfl/stops/{id} page (DS-136, tfl-stop.spec.ts and
-- the `tfl-stop` visual baseline). The api asks TFL_BASE_URL for their
-- arrivals; the frontend-e2e job points it at scripts/tfl-arrivals-stub.py,
-- which answers King's Cross St. Pancras with a fixed board and Russell
-- Square with a 503. King's Cross links its National Rail station (KGX,
-- above) through the hub crosswalk. No `tfl-*` line_status rows: they
-- would join the /status and /lines baselines, so the page lists the
-- lines as links.
-- ---------------------------------------------------------------------
INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, hub_code, latitude, longitude, fetched_at)
VALUES
  ('940GZZLUKSX', 'King''s Cross St. Pancras', 'kings cross st pancras', '{tube}',
   '[{"id": "victoria", "name": "Victoria"}, {"id": "piccadilly", "name": "Piccadilly"}]', 'HUBKGX', 51.5302, -0.1238, NOW()),
  ('940GZZLURSQ', 'Russell Square', 'russell square', '{tube}',
   '[{"id": "piccadilly", "name": "Piccadilly"}]', NULL, 51.5231, -0.1243, NOW())
ON CONFLICT (stop_id) DO NOTHING;

INSERT INTO tfl_stop_crs (stop_id, crs, source)
VALUES ('940GZZLUKSX', 'KGX', 'hub')
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------
-- A part-cancelled train (DS-181): W81181 on 2026-12-03, far from every
-- other fixture date, King's Cross to Edinburgh, left King's Cross and
-- reached York, then cancelled en route at York (TRUST 0002 at
-- fixture STANOX 99811, code IB: "Points failure (including no fault
-- found)"). Its York and Edinburgh are fixture TIPLOCs and codes (ZYK,
-- ZED) named through `tiploc_locations`, so no other page's stations or
-- names change.
-- The public train page reads "Terminates at York" with the reason, and
-- strikes Edinburgh through (e2e/train-cancellation.spec.ts, the
-- `train-part-cancelled` visual). Untracked, so no list page changes.
-- ---------------------------------------------------------------------
INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure, destination_crs, calling_points, matched_line_id, schedule_matched_at, train_id, resolved_at, created_at)
VALUES (
    'W81181', '2026-12-03', 'KGX', '2026-12-03T09:00:00Z', 'ZED',
    '[{"tiploc":"KNGX","kind":"Origin","bookedArrival":null,"bookedDeparture":"09:00:00","publicDeparture":"09:00:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSYORK","kind":"Intermediate","bookedArrival":"10:55:00","bookedDeparture":"10:57:00","publicArrival":"10:55:00","publicDeparture":"10:57:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSEDIN","kind":"Terminate","bookedArrival":"13:30:00","bookedDeparture":null,"publicArrival":"13:30:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0}]'::jsonb,
    'lner-ecml', NOW(), 'TRUST-FIXTURE-81', NOW(), NOW()
)
ON CONFLICT (train_uid, service_date) DO UPDATE SET
  train_id = EXCLUDED.train_id, calling_points = EXCLUDED.calling_points,
  matched_line_id = EXCLUDED.matched_line_id, schedule_matched_at = EXCLUDED.schedule_matched_at,
  resolved_at = EXCLUDED.resolved_at;

INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES ('99811', 'ZYK', 'ZDSYORK', 'York', 1), ('99812', 'ZED', 'ZDSEDIN', 'Edinburgh', 1)
ON CONFLICT DO NOTHING;

INSERT INTO tiploc_locations (tiploc, location_type, name, display_name, source_sequence)
VALUES ('ZDSYORK', 'station', 'York', 'York', 1), ('ZDSEDIN', 'station', 'Edinburgh', 'Edinburgh', 1)
ON CONFLICT DO NOTHING;

WITH t AS (SELECT id FROM trains WHERE train_uid = 'W81181' AND service_date = '2026-12-03')
INSERT INTO train_current_state (status, last_reported_location, last_event_type, delay_minutes, updated_at, trains_id, event_time)
SELECT 'cancelled', 'ZYK', 'ARRIVAL', 3, NOW(), t.id, '2026-12-03T10:58:00Z'
FROM t
ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET
  status = EXCLUDED.status, last_reported_location = EXCLUDED.last_reported_location,
  last_event_type = EXCLUDED.last_event_type, delay_minutes = EXCLUDED.delay_minutes,
  updated_at = EXCLUDED.updated_at;

WITH t AS (SELECT id FROM trains WHERE train_uid = 'W81181' AND service_date = '2026-12-03')
INSERT INTO train_movement_events (dedup_key, msg_type, event_type, loc_stanox, loc_crs, planned_timestamp, actual_timestamp, gbtt_timestamp, variation_status, raw_body, received_at, trains_id)
SELECT * FROM (VALUES
  ('FIXTURE-DS181-1', '0003', 'DEPARTURE', '99001', 'KGX', '2026-12-03T09:00:00Z'::timestamptz, '2026-12-03T09:01:00Z'::timestamptz, '2026-12-03T09:00:00Z'::timestamptz, 'LATE', '{}'::jsonb, NOW(), (SELECT id FROM t)),
  ('FIXTURE-DS181-2', '0003', 'ARRIVAL', '99811', 'ZYK', '2026-12-03T10:55:00Z'::timestamptz, '2026-12-03T10:58:00Z'::timestamptz, '2026-12-03T10:55:00Z'::timestamptz, 'LATE', '{}'::jsonb, NOW(), (SELECT id FROM t))
) AS v
ON CONFLICT DO NOTHING;

WITH t AS (SELECT id FROM trains WHERE train_uid = 'W81181' AND service_date = '2026-12-03')
INSERT INTO train_reasons (trains_id, msg_type, reason_code, canx_type, loc_stanox, event_at)
SELECT t.id, '0002', 'IB', 'EN ROUTE', '99811', '2026-12-03T11:05:00Z' FROM t
ON CONFLICT (trains_id, msg_type) DO UPDATE SET
  reason_code = EXCLUDED.reason_code, canx_type = EXCLUDED.canx_type,
  loc_stanox = EXCLUDED.loc_stanox, event_at = EXCLUDED.event_at;

-- ---------------------------------------------------------------------
-- A journey leg that boards and alights partway along its train
-- (journey-leg-progress.spec.ts): W90901 on 2099-02-02 calls at five
-- fixture stations, ZLA to ZLE, and fixture-member's journey 900002 rides
-- it from ZLB to ZLD. The train has left ZLA and been reported there, so
-- the leg hasn't started: its progress line shows ZLB to ZLD only, as
-- not started, and its calling points show ZLB to ZLD with ZLA and ZLE
-- behind "Earlier stops" and "Later stops" toggles. Reached through an
-- unlisted share link (the RAW token is in the spec; this row stores its
-- SHA-256 hex digest, as for the group invite above), so it appears on no
-- list page or screenshot. Fixture TIPLOCs and codes named through
-- `stanox_crs` and `tiploc_locations` only, as for W81181 above.
-- ---------------------------------------------------------------------
INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure, destination_crs, calling_points, train_id, resolved_at, created_at)
VALUES (
    'W90901', '2099-02-02', 'ZLA', '2099-02-02T09:00:00Z', 'ZLE',
    '[{"tiploc":"ZDSLPA","kind":"Origin","bookedArrival":null,"bookedDeparture":"09:00:00","publicDeparture":"09:00:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSLPB","kind":"Intermediate","bookedArrival":"09:20:00","bookedDeparture":"09:21:00","publicArrival":"09:20:00","publicDeparture":"09:21:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSLPC","kind":"Intermediate","bookedArrival":"09:40:00","bookedDeparture":"09:41:00","publicArrival":"09:40:00","publicDeparture":"09:41:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSLPD","kind":"Intermediate","bookedArrival":"10:00:00","bookedDeparture":"10:01:00","publicArrival":"10:00:00","publicDeparture":"10:01:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0},{"tiploc":"ZDSLPE","kind":"Terminate","bookedArrival":"10:20:00","bookedDeparture":null,"publicArrival":"10:20:00","isHalfMinuteArrival":false,"isHalfMinuteDeparture":false,"dayOffset":0}]'::jsonb,
    'TRUST-FIXTURE-90', NOW(), NOW()
)
ON CONFLICT (train_uid, service_date) DO UPDATE SET
  train_id = EXCLUDED.train_id, calling_points = EXCLUDED.calling_points, resolved_at = EXCLUDED.resolved_at;

INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence)
VALUES
  ('99901', 'ZLA', 'ZDSLPA', 'Fixture Ashby', 1),
  ('99902', 'ZLB', 'ZDSLPB', 'Fixture Brook', 1),
  ('99903', 'ZLC', 'ZDSLPC', 'Fixture Cross', 1),
  ('99904', 'ZLD', 'ZDSLPD', 'Fixture Dale', 1),
  ('99905', 'ZLE', 'ZDSLPE', 'Fixture End', 1)
ON CONFLICT DO NOTHING;

INSERT INTO tiploc_locations (tiploc, location_type, name, display_name, source_sequence)
VALUES
  ('ZDSLPA', 'station', 'Fixture Ashby', 'Fixture Ashby', 1),
  ('ZDSLPB', 'station', 'Fixture Brook', 'Fixture Brook', 1),
  ('ZDSLPC', 'station', 'Fixture Cross', 'Fixture Cross', 1),
  ('ZDSLPD', 'station', 'Fixture Dale', 'Fixture Dale', 1),
  ('ZDSLPE', 'station', 'Fixture End', 'Fixture End', 1)
ON CONFLICT DO NOTHING;

WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W90901' AND service_date = '2099-02-02'
)
INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_destination_crs, resolution_status, tracked_at, trains_id, notifications_enabled)
SELECT 'fixture-member', '2099-02-02', 'ZLB', 'ZLD', 'resolved', NOW(), t.id, FALSE
FROM t
WHERE NOT EXISTS (SELECT 1 FROM train_subscriptions ts WHERE ts.trains_id = t.id);

WITH t AS (
  SELECT id FROM trains WHERE train_uid = 'W90901' AND service_date = '2099-02-02'
)
INSERT INTO train_current_state (status, last_reported_location, last_event_type, delay_minutes, updated_at, trains_id, event_time)
SELECT 'en_route', 'Fixture Ashby', 'DEPARTURE', 0, NOW(), t.id, NOW()
FROM t
ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET
  status = EXCLUDED.status, last_reported_location = EXCLUDED.last_reported_location, updated_at = EXCLUDED.updated_at;

INSERT INTO journeys (id, user_id, custom_name, created_at, updated_at)
VALUES (900002, 'fixture-member', 'Fixture partway ride', NOW(), NOW())
ON CONFLICT (id) DO NOTHING;

WITH sub AS (
  SELECT ts.id FROM train_subscriptions ts
  JOIN trains t ON t.id = ts.trains_id
  WHERE t.train_uid = 'W90901' AND t.service_date = '2099-02-02' AND ts.user_id = 'fixture-member'
)
INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, train_subscription_id, match_mode)
SELECT 900002, 1, 'ZLB', 'ZLD', '2099-02-02', sub.id, 'manual'
FROM sub
ON CONFLICT (journey_id, leg_order) DO NOTHING;

INSERT INTO unlisted_links (token_hash, resource_type, resource_id, created_by, created_at, expires_at)
VALUES (encode(sha256(convert_to('fixture-leg-progress-token-12345', 'UTF8')), 'hex'), 'journey', '900002', 'fixture-member', NOW(), NOW() + INTERVAL '30 days')
ON CONFLICT DO NOTHING;
