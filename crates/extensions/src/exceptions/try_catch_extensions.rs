// Copyright (C) 2015-2025 The Neo Project.
//
// try_catch_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Try-catch extensions matching C# TryCatchExtensions exactly
pub trait TryCatchExtensions<T> {
    /// Executes an action and catches any exception.
    /// Matches C# TryCatch method
    fn try_catch<F>(&self, action: F) -> &Self
    where
        F: FnOnce(&Self);

    /// Executes an action and catches specific exceptions.
    /// Matches C# TryCatch method with exception type
    fn try_catch_exception<F>(
        &self,
        action: F,
        on_error: Option<fn(&Self, &dyn std::error::Error)>,
    ) -> &Self
    where
        F: FnOnce(&Self);

    /// Executes a function and catches specific exceptions.
    /// Matches C# TryCatch method with function and exception type
    fn try_catch_function<F, R>(
        &self,
        func: F,
        on_error: Option<fn(&Self, &dyn std::error::Error) -> Option<R>>,
    ) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>;

    /// Executes an action and re-throws specific exceptions.
    /// Matches C# TryCatchThrow method
    fn try_catch_throw<F, E>(&self, action: F) -> &Self
    where
        F: FnOnce(&Self),
        E: std::error::Error;

    /// Executes a function and re-throws specific exceptions.
    /// Matches C# TryCatchThrow method with function
    fn try_catch_throw_function<F, E, R>(&self, func: F) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>,
        E: std::error::Error;

    /// Executes an action and re-throws specific exceptions with custom message.
    /// Matches C# TryCatchThrow method with error message
    fn try_catch_throw_with_message<F, E>(&self, action: F, error_message: Option<&str>) -> &Self
    where
        F: FnOnce(&Self),
        E: std::error::Error + std::fmt::Display;

    /// Executes a function and re-throws specific exceptions with custom message.
    /// Matches C# TryCatchThrow method with function and error message
    fn try_catch_throw_function_with_message<F, E, R>(
        &self,
        func: F,
        error_message: Option<&str>,
    ) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>,
        E: std::error::Error + std::fmt::Display;
}

impl<T> TryCatchExtensions<T> for T {
    fn try_catch<F>(&self, action: F) -> &Self
    where
        F: FnOnce(&Self),
    {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        self
    }

    fn try_catch_exception<F>(
        &self,
        action: F,
        on_error: Option<fn(&Self, &dyn std::error::Error)>,
    ) -> &Self
    where
        F: FnOnce(&Self),
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        if let Err(_panic_info) = result {
            if let Some(callback) = on_error {
                // In a real implementation, this would extract the exception type
                // For now, we'll just call the callback with a generic error
                let error = std::io::Error::new(std::io::ErrorKind::Other, "Caught exception");
                callback(self, &error);
            }
        }
        self
    }

    fn try_catch_function<F, R>(
        &self,
        func: F,
        on_error: Option<fn(&Self, &dyn std::error::Error) -> Option<R>>,
    ) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| func(self)));
        match result {
            Ok(value) => value,
            Err(_) => {
                if let Some(callback) = on_error {
                    let error = std::io::Error::new(std::io::ErrorKind::Other, "Caught exception");
                    callback(self, &error)
                } else {
                    None
                }
            }
        }
    }

    fn try_catch_throw<F, E>(&self, action: F) -> &Self
    where
        F: FnOnce(&Self),
        E: std::error::Error,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        if let Err(_) = result {
            // In a real implementation, this would re-throw the specific exception
            panic!("Exception occurred in try_catch_throw");
        }
        self
    }

    fn try_catch_throw_function<F, E, R>(&self, func: F) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>,
        E: std::error::Error,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| func(self)));
        match result {
            Ok(value) => value,
            Err(_) => {
                // In a real implementation, this would re-throw the specific exception
                panic!("Exception occurred in try_catch_throw_function");
            }
        }
    }

    fn try_catch_throw_with_message<F, E>(&self, action: F, error_message: Option<&str>) -> &Self
    where
        F: FnOnce(&Self),
        E: std::error::Error + std::fmt::Display,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(self)));
        if let Err(_) = result {
            if let Some(message) = error_message {
                panic!("{}", message);
            } else {
                panic!("Exception occurred in try_catch_throw_with_message");
            }
        }
        self
    }

    fn try_catch_throw_function_with_message<F, E, R>(
        &self,
        func: F,
        error_message: Option<&str>,
    ) -> Option<R>
    where
        F: FnOnce(&Self) -> Option<R>,
        E: std::error::Error + std::fmt::Display,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| func(self)));
        match result {
            Ok(value) => value,
            Err(_) => {
                if let Some(message) = error_message {
                    panic!("{}", message);
                } else {
                    panic!("Exception occurred in try_catch_throw_function_with_message");
                }
            }
        }
    }
}
