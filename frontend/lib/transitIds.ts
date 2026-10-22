/** A network or line path segment (`metrolink`, `blue`): the api's grammar,
 * checked before asking it so a malformed URL 404s at once. */
export const TRANSIT_ID = /^[a-z0-9-]{1,32}$/;

/** A station's ATCO code (`9400ZZMAPIC`). */
export const TRANSIT_STOP_ID = /^[A-Z0-9]{1,16}$/;
