#!/bin/bash

# Fix DatabaseError
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::DatabaseError(\([^)]*\))/ExtensionError::OperationFailed(format!("Database error: {}", \1))/g' {} \;

# Fix SerializationError  
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::SerializationError(\([^)]*\))/ExtensionError::OperationFailed(format!("Serialization error: {}", \1))/g' {} \;

# Fix NetworkError
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::NetworkError(\([^)]*\))/ExtensionError::OperationFailed(format!("Network error: {}", \1))/g' {} \;

# Fix ConfigurationError
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::ConfigurationError(\([^)]*\))/ExtensionError::InvalidConfiguration(\1)/g' {} \;

# Fix AuthenticationError
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::AuthenticationError(\([^)]*\))/ExtensionError::OperationFailed(format!("Authentication error: {}", \1))/g' {} \;

# Fix PermissionError
find crates/plugins/src -name "*.rs" -exec sed -i '' 's/ExtensionError::PermissionError(\([^)]*\))/ExtensionError::OperationFailed(format!("Permission error: {}", \1))/g' {} \;

echo "Extension error replacements complete"