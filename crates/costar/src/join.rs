//! Joins a Matterport model id to its CoStar record in the hackathon kit.
//!
//! The kit's starter CSVs map model ids to a commercial `PropertyId` or a
//! residential `ListingId`; the JSON Lines files hold the records. Files are
//! streamed and pre-filtered by substring so the 400 MB kit is read once,
//! without loading it into memory.

use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;

use crate::{
    Conflict, Fact, Features, PROPERTY_SCHEMA_VERSION, PropertyKind, PropertyRecord, RecordSource,
    ScanScope, Tenant,
};

#[derive(Debug, Error)]
pub enum JoinError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("malformed {path}: {message}")]
    Malformed { path: PathBuf, message: String },
    #[error("model {0} is not in the CoStar kit's starter lists")]
    ModelNotInKit(String),
    #[error("{dataset} record {id} is missing from the kit")]
    RecordMissing { dataset: &'static str, id: i64 },
}

/// Finds the CoStar record for `model_id` in the kit at `kit`.
pub fn find_by_model_id(kit: &Path, model_id: &str) -> Result<PropertyRecord, JoinError> {
    let enums = Enums::load(&kit.join("schemas/enums.json"))?;
    if let Some(id) = csv_lookup(
        &kit.join("commercial-properties.csv"),
        "PropertyId",
        model_id,
    )? {
        return commercial(kit, &enums, id, model_id);
    }
    if let Some(id) = csv_lookup(&kit.join("residential-listings.csv"), "ListingId", model_id)? {
        return residential(kit, &enums, id, model_id);
    }
    Err(JoinError::ModelNotInKit(model_id.to_owned()))
}

fn csv_lookup(path: &Path, key: &str, model_id: &str) -> Result<Option<i64>, JoinError> {
    let file = open(path)?;
    let mut reader = csv::Reader::from_reader(file);
    let malformed = |message: String| JoinError::Malformed {
        path: path.to_owned(),
        message,
    };
    let headers = reader
        .headers()
        .map_err(|e| malformed(e.to_string()))?
        .clone();
    let column = |name: &str| {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| malformed(format!("no {name} column")))
    };
    let (key_col, model_col) = (column(key)?, column("MatterportModelId")?);
    for row in reader.records() {
        let row = row.map_err(|e| malformed(e.to_string()))?;
        if row.get(model_col) == Some(model_id) {
            let id = row.get(key_col).unwrap_or_default();
            return id
                .parse()
                .map(Some)
                .map_err(|_| malformed(format!("bad {key} {id:?}")));
        }
    }
    Ok(None)
}

fn open(path: &Path) -> Result<File, JoinError> {
    File::open(path).map_err(|source| JoinError::Io {
        path: path.to_owned(),
        source,
    })
}

/// Streams `path`, parsing only lines containing `needle`, and returns those
/// for which `keep` holds.
fn scan_jsonl(
    path: &Path,
    needle: &str,
    mut keep: impl FnMut(&Value) -> bool,
) -> Result<Vec<Value>, JoinError> {
    let mut found = Vec::new();
    for line in BufReader::new(open(path)?).lines() {
        let line = line.map_err(|source| JoinError::Io {
            path: path.to_owned(),
            source,
        })?;
        if !line.contains(needle) {
            continue;
        }
        let value: Value = serde_json::from_str(&line).map_err(|e| JoinError::Malformed {
            path: path.to_owned(),
            message: e.to_string(),
        })?;
        if keep(&value) {
            found.push(value);
        }
    }
    Ok(found)
}

fn record_by_id(path: &Path, id: i64) -> Result<Option<Value>, JoinError> {
    Ok(scan_jsonl(path, &format!("\"id\":{id}"), |v| {
        v["id"].as_i64() == Some(id)
    })?
    .into_iter()
    .next())
}

struct Enums(HashMap<String, HashMap<String, String>>);

impl Enums {
    fn load(path: &Path) -> Result<Self, JoinError> {
        serde_json::from_reader(BufReader::new(open(path)?))
            .map(Self)
            .map_err(|e| JoinError::Malformed {
                path: path.to_owned(),
                message: e.to_string(),
            })
    }

    /// Decodes an integer enum; `0` (Unknown/None) and unknown values are `None`.
    fn decode(&self, name: &str, value: &Value) -> Option<String> {
        let n = value.as_i64().filter(|n| *n != 0)?;
        self.0.get(name)?.get(&n.to_string()).cloned()
    }
}

/// Collects facts for display as fields are read.
#[derive(Default)]
struct Facts(Vec<Fact>);

impl Facts {
    fn add<T: ToString>(&mut self, label: &str, value: Option<T>, field: &str) {
        if let Some(value) = value {
            self.0.push(Fact {
                label: label.to_owned(),
                value: value.to_string(),
                source_field: field.to_owned(),
            });
        }
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
fn u(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}
fn yes_no(v: Option<bool>) -> Option<&'static str> {
    v.map(|b| if b { "Yes" } else { "No" })
}
fn date(v: &Value) -> Option<String> {
    s(v).map(|d| d.chars().take(10).collect())
}

fn residential(
    kit: &Path,
    enums: &Enums,
    listing_id: i64,
    model_id: &str,
) -> Result<PropertyRecord, JoinError> {
    let listing = record_by_id(&kit.join("data/residential-listings.jsonl"), listing_id)?.ok_or(
        JoinError::RecordMissing {
            dataset: "residential-listings",
            id: listing_id,
        },
    )?;
    let features_record = record_by_id(
        &kit.join("data/residential-listing-features.jsonl"),
        listing_id,
    )?;
    let p = &listing["property"];
    let addr = &p["address"];
    let reso = &listing["resoValues"];
    let structure = &reso["structure"];
    let costar = &listing["costarFeatureValues"];

    let year_built = p["yearBuilt"]
        .as_i64()
        .or_else(|| structure["yearBuilt"].as_i64())
        .and_then(|y| i32::try_from(y).ok());
    let stories = u(&p["stories"]).or_else(|| u(&structure["stories"]));
    let area = p["sqftMax"].as_f64().or_else(|| p["sqftMin"].as_f64());
    let bedrooms = u(&p["bedsMax"]).or_else(|| u(&p["bedsMin"]));
    let bathrooms = p["bathsMax"].as_f64();
    let features = Features {
        fireplace: reso["hasFireplace"]
            .as_bool()
            .or(costar["hasFireplace"].as_bool()),
        fireplaces: u(&structure["fireplacesTotal"]),
        hardwood_floors: reso["hasHardwoodFloors"]
            .as_bool()
            .or(costar["hasHardwoodFloors"].as_bool()),
        basement: reso["hasBasement"]
            .as_bool()
            .or(costar["hasBasement"].as_bool()),
        garage: reso["hasGarage"]
            .as_bool()
            .or(costar["hasGarage"].as_bool()),
        architectural_style: s(&structure["architecturalStyle"]),
        accessibility_features: reso["hasAccessibilityFeatures"].as_bool(),
    };
    let conflicts = [
        "hasGarage",
        "hasFireplace",
        "hasBasement",
        "hasHardwoodFloors",
    ]
    .into_iter()
    .filter_map(|field| {
        let (a, b) = (reso[field].as_bool()?, costar[field].as_bool()?);
        (a != b).then(|| Conflict {
            label: field.trim_start_matches("has").to_owned(),
            values: vec![
                fact(field, a, &format!("resoValues.{field}")),
                fact(field, b, &format!("costarFeatureValues.{field}")),
            ],
        })
    })
    .collect();

    let mut f = Facts::default();
    f.add(
        "Address",
        s(&addr["fullStreetAddress"]),
        "property.address.fullStreetAddress",
    );
    f.add(
        "Home type",
        s(&p["mlsPropertyType"]),
        "property.mlsPropertyType",
    );
    f.add("Year built", year_built, "property.yearBuilt");
    f.add("Stories", stories, "property.stories");
    f.add("Living area (sq ft)", area, "property.sqftMax");
    f.add("Bedrooms", bedrooms, "property.bedsMax");
    f.add("Bathrooms", bathrooms, "property.bathsMax");
    f.add(
        "Style",
        features.architectural_style.clone(),
        "resoValues.structure.architecturalStyle",
    );
    f.add(
        "Fireplace",
        yes_no(features.fireplace),
        "resoValues.hasFireplace",
    );
    f.add(
        "Hardwood floors",
        yes_no(features.hardwood_floors),
        "resoValues.hasHardwoodFloors",
    );
    f.add(
        "Basement",
        yes_no(features.basement),
        "resoValues.hasBasement",
    );
    f.add(
        "Elevator",
        yes_no(reso["hasElevator"].as_bool()),
        "resoValues.hasElevator",
    );
    f.add(
        "Parking spaces",
        u(&p["totalParkingSpaces"]),
        "property.totalParkingSpaces",
    );
    if let Some(record) = &features_record {
        for line in record["highlights"].as_array().into_iter().flatten() {
            f.add(
                "Listing highlight",
                s(&line["text"]),
                "residential-listing-features.highlights",
            );
        }
    }

    Ok(PropertyRecord {
        schema_version: PROPERTY_SCHEMA_VERSION,
        source: RecordSource {
            provider: "costar".into(),
            dataset: "residential-listings".into(),
            record_id: listing_id,
            as_of: date(&listing["listDate"]),
        },
        kind: PropertyKind::Residential,
        scan: ScanScope {
            model_id: model_id.to_owned(),
            description: Some("Whole-home 3D tour on the MLS listing".into()),
            partial: false,
            captured: None,
        },
        name: None,
        address: s(&addr["fullStreetAddress"]),
        city: s(&addr["city"]),
        state: s(&addr["state"]),
        latitude: addr["latitude"].as_f64(),
        longitude: addr["longitude"].as_f64(),
        property_type: s(&p["mlsPropertyType"])
            .or_else(|| enums.decode("ListingPropertyType", &p["listingPropertyType"])),
        year_built,
        stories,
        area_sqft: area,
        bedrooms,
        bathrooms,
        construction_material: None,
        has_sprinklers: None,
        sprinkler_type: None,
        has_elevators: reso["hasElevator"].as_bool(),
        is_walkup: None,
        units: None,
        buildings: None,
        features,
        tenants: Vec::new(),
        conflicts,
        facts: f.0,
    })
}

fn fact(label: &str, value: impl ToString, field: &str) -> Fact {
    Fact {
        label: label.to_owned(),
        value: value.to_string(),
        source_field: field.to_owned(),
    }
}

fn commercial(
    kit: &Path,
    enums: &Enums,
    property_id: i64,
    model_id: &str,
) -> Result<PropertyRecord, JoinError> {
    let p = record_by_id(&kit.join("data/commercial-property.jsonl"), property_id)?.ok_or(
        JoinError::RecordMissing {
            dataset: "commercial-property",
            id: property_id,
        },
    )?;
    let addr = &p["primaryAddress"];
    let apt = &p["propertyApartment"];
    let street = [
        &addr["streetNumber"],
        &addr["streetName"],
        &addr["streetType"],
    ]
    .into_iter()
    .filter_map(s)
    .collect::<Vec<_>>()
    .join(" ");
    let address = (!street.is_empty()).then_some(street);
    let year_built = p["constructionEvents"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["eventType"].as_i64() == Some(2))
        .and_then(|e| e["year"].as_i64())
        .and_then(|y| i32::try_from(y).ok());
    let area = p["rentableBuildingAreaSquareFeet"]
        .as_f64()
        .or_else(|| p["grossBuildingAreaSquareFeet"].as_f64());
    let property_type = enums.decode("PropertyType", &p["propertyType"]);
    let construction = enums.decode(
        "BuildingConstructionMaterialType",
        &p["buildingConstructionMaterial"],
    );
    let sprinkler_type = enums.decode("SprinklerType", &p["sprinklerType"]);
    let has_sprinklers = p["hasSprinklers"]
        .as_bool()
        .or(sprinkler_type.as_ref().map(|_| true));
    let has_elevators = elevators(&p);
    let (scan_caption, captured) = scan_attachment(&p, model_id);

    let mut tenants = current_tenants(kit, enums, property_id)?;
    tenants.sort_by_key(|t| std::cmp::Reverse(t.employees));

    let mut f = Facts::default();
    f.add("Building", s(&p["buildingName"]), "buildingName");
    f.add("Address", address.clone(), "primaryAddress");
    f.add("Property type", property_type.clone(), "propertyType");
    f.add(
        "Year built",
        year_built,
        "constructionEvents[ConstructionComplete].year",
    );
    f.add(
        "Stories above grade",
        u(&p["numStoriesAboveGrade"]),
        "numStoriesAboveGrade",
    );
    f.add(
        "Building area (sq ft)",
        area,
        "rentableBuildingAreaSquareFeet",
    );
    f.add(
        "Construction",
        construction.clone(),
        "buildingConstructionMaterial",
    );
    f.add(
        "Sprinklers",
        Some(match has_sprinklers {
            Some(true) => sprinkler_type.clone().unwrap_or_else(|| "Yes".into()),
            Some(false) => "No".into(),
            None => "Unknown".into(),
        }),
        "hasSprinklers / sprinklerType",
    );
    f.add(
        "Elevators",
        Some(yes_no(has_elevators).unwrap_or("Unknown")),
        "structural.elevators",
    );
    f.add(
        "Walk-up",
        yes_no(apt["isWalkup"].as_bool()),
        "propertyApartment.isWalkup",
    );
    f.add(
        "Units",
        u(&apt["totalNumUnits"]),
        "propertyApartment.totalNumUnits",
    );
    f.add(
        "Buildings",
        u(&apt["numBuildings"]),
        "propertyApartment.numBuildings",
    );
    f.add(
        "Current tenants",
        Some(tenants.len()),
        "location-occupancy (toDate = null)",
    );

    Ok(PropertyRecord {
        schema_version: PROPERTY_SCHEMA_VERSION,
        source: RecordSource {
            provider: "costar".into(),
            dataset: "commercial-property".into(),
            record_id: property_id,
            as_of: captured.clone(),
        },
        kind: PropertyKind::Commercial,
        scan: ScanScope {
            model_id: model_id.to_owned(),
            description: scan_caption,
            // A commercial scan is a space inside the property, not the property.
            partial: true,
            captured,
        },
        name: s(&p["buildingName"]),
        address,
        city: s(&addr["city"]),
        state: s(&addr["state"]),
        latitude: addr["latitude"].as_f64(),
        longitude: addr["longitude"].as_f64(),
        property_type,
        year_built,
        stories: u(&p["numStoriesAboveGrade"]),
        area_sqft: area,
        bedrooms: None,
        bathrooms: None,
        construction_material: construction,
        has_sprinklers,
        sprinkler_type,
        has_elevators,
        is_walkup: apt["isWalkup"].as_bool(),
        units: u(&apt["totalNumUnits"]),
        buildings: u(&apt["numBuildings"]),
        features: Features::default(),
        tenants,
        conflicts: Vec::new(),
        facts: f.0,
    })
}

/// `Some(true)` if any passenger or freight elevator exists, `Some(false)` if
/// every recorded one is marked as not existing, `None` when unknown.
fn elevators(p: &Value) -> Option<bool> {
    let lists = [
        &p["propertyApartment"]["elevators"],
        &p["propertyOffice"]["structural"]["elevators"],
        &p["propertyRetail"]["structural"]["elevators"],
        &p["propertyIndustrial"]["structural"]["elevators"],
        &p["propertyFlex"]["structural"]["elevators"],
    ];
    let mut known = None;
    for e in lists.into_iter().filter_map(Value::as_array).flatten() {
        let lift = matches!(e["elevatorType"].as_i64(), Some(1 | 2));
        match e["elevatorState"].as_i64() {
            Some(1) if lift => return Some(true),
            Some(2) if lift => known = Some(false),
            _ => {}
        }
    }
    known
}

/// The caption and capture date of the attachment carrying this model.
fn scan_attachment(p: &Value, model_id: &str) -> (Option<String>, Option<String>) {
    let mut lists = vec![&p["attachments"]];
    for model in p["propertyApartment"]["models"]
        .as_array()
        .into_iter()
        .flatten()
    {
        lists.push(&model["attachments"]);
        for unit in model["units"].as_array().into_iter().flatten() {
            lists.push(&unit["attachments"]);
        }
    }
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .find(|a| a["videoKey"].as_str() == Some(model_id))
        .map(|a| (s(&a["caption"]), date(&a["originationDate"])))
        .unwrap_or_default()
}

fn current_tenants(kit: &Path, enums: &Enums, property_id: i64) -> Result<Vec<Tenant>, JoinError> {
    let occupancies = scan_jsonl(
        &kit.join("data/location-occupancy.jsonl"),
        &format!("\"propertyId\":{property_id}"),
        |o| o["propertyId"].as_i64() == Some(property_id) && o["toDate"].is_null(),
    )?;
    let ids: BTreeSet<i64> = occupancies
        .iter()
        .filter_map(|o| o["tenantLocationId"].as_i64())
        .collect();
    let unknown = occupancies.len()
        - occupancies
            .iter()
            .filter(|o| o["tenantLocationId"].is_i64())
            .count();
    let companies = scan_jsonl(&kit.join("data/company-detail.jsonl"), "\"id\":", |c| {
        c["id"].as_i64().is_some_and(|id| ids.contains(&id))
    })?;
    let mut tenants: Vec<Tenant> = companies
        .iter()
        .map(|c| Tenant {
            name: s(&c["name"]),
            industry: enums.decode("IndustryType", &c["profile"]["industryType"]),
            employees: u(&c["numEmployees"]),
        })
        .collect();
    tenants.extend((0..unknown).map(|_| Tenant {
        name: None,
        industry: None,
        employees: None,
    }));
    Ok(tenants)
}
