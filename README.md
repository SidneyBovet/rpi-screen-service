# Raspi Screen Service

Client and server apps to run my LED panel from a Raspberry Pi, with another, more capable machine doing the heavylifting of various APIs.

## TODO

- [x] add Kitty parser
- [x] implement proper logging
- [x] move to prost 0.13
- [ ] find a way to have led matrix only for the Rpi client
- [x] make another client that just prints the proto
- [x] have the cli client query hash and do the full request on changes
- [x] play with `google_calendar` crate to read stuff from gCal
- [x] Query stop info from [Open Transport Data](https://opentransportdata.swiss/en/cookbook/open-journey-planner-ojp/) (Timonet ID: 8588845)
- [ ] Test the gCal and transport updaters in Real mode
- [ ] Get enough results that we have next to Flon, and next to Renens (could be 32 or 54)
- [ ] Check out how to POST, and parse XML
- [ ] Play with [sunrise / sunset API](https://sunrise-sunset.org/api) to have auto brightness

### To document

- [ ] How to build: features for the server, etc.
- [ ] The log4rs config + default log file location
- [ ] The API config (esp. since it's not checked in)