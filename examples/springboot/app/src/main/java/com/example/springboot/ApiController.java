package com.example.springboot;

import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;

import org.springframework.http.MediaType;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class ApiController {

    @GetMapping(value = "/", produces = MediaType.APPLICATION_JSON_VALUE)
    public Map<String, Object> root() {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("message", "Hello from Spring Boot on Azure Functions!");
        response.put("framework", "Spring Boot");
        response.put("adapter", "Azure Functions Web Adapter");
        response.put("timestamp", Instant.now().toString());
        return response;
    }

    @GetMapping(value = "/api/hello", produces = MediaType.APPLICATION_JSON_VALUE)
    public Map<String, String> hello(@RequestParam(defaultValue = "World") String name) {
        return Map.of("message", "Hello, " + name + "!");
    }

    @PostMapping(value = "/api/echo", produces = MediaType.APPLICATION_JSON_VALUE)
    public Map<String, Object> echo(
        @RequestBody(required = false) Map<String, Object> body,
        @RequestHeader Map<String, String> headers
    ) {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("received", body == null ? Map.of() : body);
        response.put("headers", headers);
        return response;
    }

    @GetMapping(value = "/api/health", produces = MediaType.APPLICATION_JSON_VALUE)
    public Map<String, String> health() {
        return Map.of("status", "healthy");
    }
}