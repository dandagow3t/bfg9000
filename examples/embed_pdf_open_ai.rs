use dotenv::dotenv;
use lopdf::Document as LoPdfDocument;
use mongodb::{
    bson::{self, doc},
    options::ClientOptions,
    Client as MongoClient, Collection,
};
use pdf_extract::extract_text;
use rig::{
    embeddings::EmbeddingsBuilder,
    providers::{eternalai::TEXT_EMBEDDING_ADA_002, openai::Client},
    vector_store::VectorStoreIndex,
    Embed,
};
use rig_derive::Embed;
use rig_mongodb::{MongoDbVectorIndex, SearchParams};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::env;
use std::error::Error;
use std::path::Path;
use tokio;

#[derive(Embed, Debug, Serialize, Deserialize)]
struct Paragraph {
    #[embed]
    content: String,
    #[serde(rename = "_id", deserialize_with = "deserialize_object_id")]
    id: String,
}

fn deserialize_object_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Object(map) => {
            if let Some(Value::String(oid)) = map.get("$oid") {
                Ok(oid.clone())
            } else {
                Err(serde::de::Error::custom(
                    "Expected $oid field with string value",
                ))
            }
        }
        _ => Err(serde::de::Error::custom(
            "Expected string or object with $oid field",
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Document {
    paragraphs: Vec<Paragraph>,
    metadata: DocumentMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
struct DocumentMetadata {
    filename: String,
    total_pages: i32,
}

fn get_total_pages(filepath: &str) -> Result<i32, Box<dyn Error>> {
    let document = LoPdfDocument::load(filepath)?;
    Ok(document.get_pages().len() as i32)
}

async fn process_pdf<P: AsRef<Path>>(filepath: P) -> Result<Document, Box<dyn Error>> {
    let path = filepath.as_ref();
    tracing::info!("Processing PDF: {:?}", &path);
    let text = extract_text(path)?;

    // Split text into lines first
    let lines: Vec<&str> = text.lines().collect();
    let mut paragraphs = Vec::new();
    let mut current_paragraph = String::new();
    let mut current_page = 1;
    let mut id = 0;
    for (_, line) in lines.iter().enumerate() {
        let line = line.trim();
        // Check for page number (standalone number)
        if !line.is_empty() && line.chars().all(|c| c.is_digit(10)) {
            current_page = line.parse().unwrap_or(current_page);
            tracing::info!("Found page number: {:?}", current_page);
            continue;
        }

        // If line is empty and we have content, save paragraph
        if line.is_empty() && !current_paragraph.is_empty() {
            tracing::info!("Found paragraph: {:?}", current_paragraph);
            paragraphs.push(Paragraph {
                content: current_paragraph.trim().to_string(),
                id: format!("{}_{}", current_page, id),
            });
            current_paragraph.clear();
        } else if !line.is_empty() {
            // Add non-empty lines to current paragraph
            if !current_paragraph.is_empty() {
                current_paragraph.push(' ');
            }
            current_paragraph.push_str(line);
        }
        id += 1;
    }

    // Don't forget the last paragraph
    if !current_paragraph.is_empty() {
        paragraphs.push(Paragraph {
            content: current_paragraph.trim().to_string(),
            id: format!("{}_{}", current_page, id),
        });
    }

    tracing::info!("Found {} paragraphs", paragraphs.len());

    Ok(Document {
        paragraphs,
        metadata: DocumentMetadata {
            filename: path.to_string_lossy().to_string(),
            total_pages: get_total_pages(path.to_str().unwrap())?,
        },
    })
}

static DB_NAME: &str = "rig_knowledgebase";
static COLLECTION_NAME: &str = "context";
static VECTOR_INDEX_NAME: &str = "vector_index";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let openai_api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let openai_client = Client::new(&openai_api_key);

    // Initialize MongoDB client
    let mongodb_connection_string = env::var("MONGODB_CONNECTION_STRING")
        .expect("MONGMONGODB_CONNECTION_STRINGODB_URI not set");
    let options = ClientOptions::parse(mongodb_connection_string)
        .await
        .expect("MongoDB connection string should be valid");

    let mongodb_client =
        MongoClient::with_options(options).expect("MongoDB client options should be valid");

    // Check if the database already exists
    let db_exists = mongodb_client
        .list_database_names()
        .await?
        .contains(&DB_NAME.to_string());
    // Initialize MongoDB vector store
    let collection: Collection<bson::Document> =
        mongodb_client.database(DB_NAME).collection(COLLECTION_NAME);

    // Select the embedding model and generate our embeddings
    let model = openai_client.embedding_model(TEXT_EMBEDDING_ADA_002);

    match db_exists {
        false => {
            let current_dir = env::current_dir()?;
            let documents_dir = current_dir.join("documents");

            let filepath = documents_dir.join("The-Complete-Guide-to-Trading.pdf");

            let document = process_pdf(filepath).await?;
            // Create the embeddings
            let embeddings = EmbeddingsBuilder::new(model.clone())
                .documents(document.paragraphs)?
                .build()
                .await?;

            let mongo_documents = embeddings
                .iter()
                .map(|(Paragraph { content, id, .. }, embedding)| {
                    doc! {
                        "id": id.clone(),
                        "content": content.clone(),
                        "embedding": embedding.first().vec.clone(),
                    }
                })
                .collect::<Vec<_>>();

            match collection.insert_many(mongo_documents).await {
                Ok(_) => println!("Documents added successfully"),
                Err(e) => println!("Error adding documents: {:?}", e),
            };
        }
        true => {
            println!(
                "Database '{}' already exists. Skipping population.",
                DB_NAME
            );
            // Create a vector index on our vector store.
            // Note: a vector index called "vector_index" must exist on the MongoDB collection you are querying.
            // IMPORTANT: Reuse the same model that was used to generate the embeddings
            let index =
                MongoDbVectorIndex::new(collection, model, VECTOR_INDEX_NAME, SearchParams::new())
                    .await?;

            let size = 11;
            let test_query = "What is stock investing - value investing?";
            // Query the index
            let results = index.top_n::<Paragraph>(test_query, size).await?;

            println!("Results");
            results.iter().for_each(|(score, _, paragraph)| {
                println!("{:?}, {:?}", score, paragraph.content);
            });
            let id_results = index.top_n_ids(test_query, size).await?;

            println!("ID results: {:?}", id_results);
        }
    }

    Ok(())
}
